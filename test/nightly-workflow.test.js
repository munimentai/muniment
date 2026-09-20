import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { macosSigningProvenance } from '../.github/lib/release-promotion.mjs'

const workflow = fs.readFileSync('.github/workflows/nightly.yml', 'utf8')
const ensureJunitReport = 'test/e2e/support/ensure-junit-report.sh'

// This helper needs a POSIX shell, and Windows has none.
const runReportFallback = (directory, suite, runStatus, extractStatus, createSuccessReport = false) => {
  const result = spawnSync('bash', [ensureJunitReport, directory, suite, String(runStatus), String(extractStatus), createSuccessReport ? '1' : '0'], { encoding: 'utf8' })
  expect(result.status, result.stderr).toBe(0)
}
const linuxRunner = fs.readFileSync('test/e2e/runner/linux.sh', 'utf8')
const publish = workflow.slice(workflow.indexOf('  publish:'), workflow.indexOf('  linux-e2e:'))
const linuxE2e = workflow.slice(workflow.indexOf('  linux-e2e:'), workflow.indexOf('  windows-e2e:'))
const job = (name, nextName) => workflow.slice(
  workflow.indexOf(`  ${name}:`),
  nextName ? workflow.indexOf(`  ${nextName}:`) : workflow.length,
)
const conditionFor = (jobText) => jobText.match(/    if: >-\n((?:      .+\n)+)/)[1].trim().replace(/\n\s*/g, ' ')
const evaluateCondition = (condition, { eventName, platform, cancelled = false, prepare = 'success', previous = {} }) => Function(
  `"use strict"; return (${condition
    .replace('cancelled()', JSON.stringify(cancelled))
    .replace('always()', 'true')
    .replaceAll('needs.prepare.result', JSON.stringify(prepare))
    .replaceAll('needs.linux-e2e.result', JSON.stringify(previous.linux))
    .replaceAll('needs.windows-e2e.result', JSON.stringify(previous.windows))
    .replaceAll('github.event_name', JSON.stringify(eventName))
    .replaceAll('github.event.inputs.platform', JSON.stringify(platform))})`,
)()
const jobCondition = linuxE2e.match(/    if: >-\n((?:      .+\n)+)/)[1].trim().replace(/\n\s*/g, ' ')
const conditionResult = ({ eventName, platform, cancelled = false, prepare = 'success' }) => {
  const expression = jobCondition
    .replace('cancelled()', JSON.stringify(cancelled))
    .replace('always()', 'true')
    .replaceAll('needs.prepare.result', JSON.stringify(prepare))
    .replaceAll('github.event_name', JSON.stringify(eventName))
    .replaceAll('github.event.inputs.platform', JSON.stringify(platform))
  return Function(`"use strict"; return (${expression})`)()
}

describe('nightly macOS package build', () => {
  it.skipIf(process.platform === 'win32')('passes the remaining full build budget after Git and dependency setup', () => {
    const commands = workflow.slice(workflow.indexOf("          build='"), workflow.indexOf('          repo_url='))
    const result = spawnSync('bash', ['-c', `
      set -eu
      PLATFORM=macos
      SOURCE_SHA=test-sha
      REPOSITORY=test/repo
      date() { echo 'The host must not start the build clock.' >&2; return 1; }
      ${commands}
      elapsed=0
      date() { echo $((1000000 + elapsed)); }
      git() { elapsed=$((elapsed + 100)); }
      npm() { elapsed=$((elapsed + 300)); }
      rustup() { elapsed=$((elapsed + 200)); }
      .() {
        [ "$1" = scripts/prepare-cef-macos.sh ] || return 1
        elapsed=$((elapsed + 50))
      }
      node() {
        if [ "$1" = .github/build-macos-app.mjs ]; then
          printf '%s %s\\n' "$elapsed" "$MACOS_BUILD_REMAINING_SECONDS"
        fi
      }
      eval "$cmd"
    `], { encoding: 'utf8' })
    expect(result.stderr).toBe('')
    expect(result.status).toBe(0)
    expect(result.stdout).toBe('750 2850\n')
  })

  it('builds and publishes the package while signing stays disabled', () => {
    expect(workflow).toContain('node .github/build-macos-app.mjs')
    expect(workflow).toContain('[".pkg"]')
    expect(workflow).toContain("MACOS_SIGNING_ENABLED: ${{ vars.MACOS_SIGNING_ENABLED || 'false' }}")
    expect(workflow).toContain(macosSigningProvenance('${sha}').replaceAll('`', '\\`'))
    expect(workflow).toContain('macOS artifacts are unsigned pending Apple enrollment Y5DUNHQA74.')
  })
})

describe('nightly Linux E2E workflow', () => {
  it('finalizes with contents write without moving the rolling tag', () => {
    expect(publish).toContain('permissions:\n      contents: write')
    expect(publish).not.toContain('target_commitish: sha')
    expect(publish).not.toContain('github.rest.git.updateRef')
  })

  it('runs after a successful full-nightly publish', () => {
    expect(conditionResult({ eventName: 'schedule', platform: '' })).toBe(true)
  })

  it('runs after a failed full-nightly publish', () => {
    expect(jobCondition).not.toContain('needs.publish.result')
    expect(publish).not.toContain('continue-on-error')
    expect(conditionResult({ eventName: 'schedule', platform: '' })).toBe(true)
  })

  it('runs a Linux-only dispatch and validates its asset in the report-producing runner', () => {
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'linux' })).toBe(true)
    expect(linuxE2e).not.toContain('github.rest.git.updateRef')
    expect(linuxRunner).toContain('asset-identity.mjs "$sha"')
  })

  it('runs after another matrix build fails', () => {
    expect(jobCondition).not.toContain('needs.build.result')
    expect(conditionResult({ eventName: 'schedule', platform: '' })).toBe(true)
  })

  it('does not run for another targeted platform or a failed prepare job', () => {
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'windows' })).toBe(false)
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'linux', prepare: 'failure' })).toBe(false)
  })

  it('requires verified MinIO evidence for every platform without GitHub artifact storage', () => {
    expect(workflow).not.toMatch(/actions\/(?:upload|download)-artifact|actions\/artifacts/)
    for (const platform of ['linux', 'windows', 'macos']) {
      const lane = job(`${platform}-e2e`, platform === 'linux' ? 'windows-e2e' : platform === 'windows' ? 'macos-e2e' : 'verify-requested-e2e')
      const step = lane.slice(lane.indexOf('      - name: Publish diagnostics and JUnit to MinIO'), lane.indexOf('      - name: Preserve E2E result'))
      expect(step).toContain('if: always()')
      expect(step).not.toContain('continue-on-error')
      expect(step).toContain('AWS_ACCESS_KEY_ID: ${{ secrets.FACTORY_CI_S3_ACCESS_KEY }}')
      expect(step).toContain('AWS_SECRET_ACCESS_KEY: ${{ secrets.FACTORY_CI_S3_SECRET_KEY }}')
      expect(step).toContain(`bash .github/publish-ci-artifacts.sh ${platform}`)
    }
  })

  it('requests the desktop-ci artifact collector for the guest-published report', () => {
    const runStep = linuxE2e.slice(linuxE2e.indexOf('      - name: Run installed Linux sign-in via desktop-ci'), linuxE2e.indexOf('      - name: Publish diagnostics and JUnit to MinIO'))
    expect(runStep).toContain('sudo desktop-ci linux')
    expect(runStep).toContain('--collect-artifacts')
    expect(runStep).toContain('extract-artifacts.sh')
  })

  // desktop-ci resolves --ref with init+fetch-by-ref, so it takes a branch or a
  // commit; ADR 0013 requires the installed lanes to pin the exact SHA their
  // artifacts were built from rather than whatever main points at by then.
  it('pins every installed-E2E lane to the source SHA at --ref', () => {
    expect(workflow.match(/--ref '\$SOURCE_SHA'/g)).toHaveLength(3)
    for (const lane of ['linux', 'windows', 'macos']) {
      expect(workflow).toContain(`sudo desktop-ci ${lane} --repo 'https://github.com/\${REPOSITORY}.git' --ref '$SOURCE_SHA'`)
    }
  })

  it('checks out the pinned E2E harness in every installed lane', () => {
    expect(workflow.match(/- name: Check out E2E harness/g)).toHaveLength(3)
    expect(workflow.match(/ref: \$\{\{ needs\.prepare\.outputs\.source_sha \}\}/g)).toHaveLength(3)
  })

  it('uses the SSH key provided by the self-hosted runner for every E2E lane', () => {
    expect(workflow).not.toContain('DESKTOP_CI_SSH_KEY: ${{ secrets.DESKTOP_CI_SSH_KEY }}')
    expect(workflow.match(/printf '%s' "\$DESKTOP_CI_SSH_KEY"/g)).toHaveLength(4)
  })

  it('does not pass a cloud provider credential to installed E2E lanes', () => {
    expect(workflow).not.toContain('DESKTOP_E2E_PROVIDER_KEY')
    expect(workflow).not.toContain('MUNIMENT_E2E_PROVIDER_KEY')
  })
})

describe('nightly Windows E2E workflow', () => {
  const start = workflow.indexOf('  windows-e2e:')
  const windowsE2e = workflow.slice(start, workflow.indexOf('  macos-e2e:', start))

  it.skipIf(process.platform === 'win32')('publishes a parseable infrastructure failure when an extracted failure has no JUnit', () => {
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
    fs.writeFileSync(path.join(artifacts, 'desktop-ci.log'), 'setup failed')

    runReportFallback(artifacts, 'installed-windows', 1, 0)

    const report = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
    const xml = new DOMParser().parseFromString(report, 'application/xml')
    expect(report).toMatch(/^<\?xml version="1\.0" encoding="UTF-8"\?>/)
    expect(xml.querySelector('parsererror')).toBeNull()
    expect(xml.querySelectorAll('testcase')).toHaveLength(1)
    expect(xml.querySelectorAll('failure')).toHaveLength(1)
    expect(report).toContain('<testsuites tests="1" failures="1">')
    expect(report).toContain('<testsuite name="installed-windows" tests="1" failures="1">')
    expect(report).toContain('<testcase name="desktop-ci infrastructure">')
    expect(report).toContain('<failure message="desktop-ci failed before producing a JUnit report"/>')
    expect(fs.readFileSync(path.join(artifacts, 'desktop-ci.log'), 'utf8')).toBe('setup failed')
  })
})

describe('nightly macOS E2E workflow', () => {
  const start = workflow.indexOf('  macos-e2e:')
  const macosE2e = workflow.slice(start)

  it('is serialized after Windows and requests collection plus a screendump', () => {
    expect(start).toBeGreaterThan(-1)
    expect(macosE2e).toContain('needs: [prepare, windows-e2e]')
    expect(macosE2e).toContain('needs.windows-e2e.result')
    expect(macosE2e).toContain('sudo desktop-ci macos')
    expect(macosE2e).toContain('--env-stdin')
    expect(macosE2e).toContain('--collect-artifacts --screendump')
  })

  it('validates the pinned SHA and sends the sign-in fixture credentials', () => {
    expect(macosE2e).toContain('^[0-9a-f]{40}$')
    expect(macosE2e).toContain('DESKTOP_E2E_USERNAME')
    expect(macosE2e).toContain('DESKTOP_E2E_PASSWORD')
    expect(macosE2e).toContain('MUNIMENT_E2E_USERNAME')
    expect(macosE2e).toContain('MUNIMENT_E2E_PASSWORD')
  })


})

describe('nightly targeted E2E dispatch', () => {
  const lanes = [
    ['linux', job('linux-e2e', 'windows-e2e')],
    ['windows', job('windows-e2e', 'macos-e2e')],
    ['macos', job('macos-e2e', 'verify-requested-e2e')],
  ]

  it.each(lanes)('accepts the %s platform in its job condition', (platform, lane) => {
    expect(conditionFor(lane)).toContain(`github.event.inputs.platform == '${platform}'`)
  })

  it('runs targeted Windows and macOS jobs after earlier jobs skip', () => {
    expect(evaluateCondition(conditionFor(lanes[1][1]), {
      eventName: 'workflow_dispatch', platform: 'windows', previous: { linux: 'skipped' },
    })).toBe(true)
    expect(evaluateCondition(conditionFor(lanes[2][1]), {
      eventName: 'workflow_dispatch', platform: 'macos', previous: { windows: 'skipped' },
    })).toBe(true)
  })

  it.each(lanes)('does not run the %s lane after the workflow is cancelled', (_platform, lane) => {
    expect(evaluateCondition(conditionFor(lane), {
      eventName: 'schedule', platform: '', cancelled: true,
      previous: { linux: 'success', windows: 'success' },
    })).toBe(false)
  })

  it('keeps all-platform jobs ordered through their dependencies and skip checks', () => {
    const windows = lanes[1][1]
    const macos = lanes[2][1]
    expect(windows).toContain('needs: [prepare, linux-e2e]')
    expect(conditionFor(windows)).toContain("github.event.inputs.platform == 'all'")
    expect(conditionFor(windows)).toContain("needs.linux-e2e.result != 'skipped'")
    expect(macos).toContain('needs: [prepare, windows-e2e]')
    expect(conditionFor(macos)).toContain("github.event.inputs.platform == 'all'")
    expect(conditionFor(macos)).toContain("needs.windows-e2e.result != 'skipped'")
    expect(evaluateCondition(conditionFor(windows), {
      eventName: 'workflow_dispatch', platform: 'all', previous: { linux: 'success' },
    })).toBe(true)
    expect(evaluateCondition(conditionFor(windows), {
      eventName: 'workflow_dispatch', platform: 'all', previous: { linux: 'skipped' },
    })).toBe(false)
    expect(evaluateCondition(conditionFor(macos), {
      eventName: 'workflow_dispatch', platform: 'all', previous: { windows: 'success' },
    })).toBe(true)
    expect(evaluateCondition(conditionFor(macos), {
      eventName: 'workflow_dispatch', platform: 'all', previous: { windows: 'skipped' },
    })).toBe(false)
  })

  it('fails a targeted dispatch when its requested E2E job was skipped', () => {
    const verification = job('verify-requested-e2e')
    expect(verification).toContain('needs: [linux-e2e, windows-e2e, macos-e2e]')
    expect(verification).toContain("if: always() && github.event_name == 'workflow_dispatch' && github.event.inputs.platform != 'all'")
    expect(verification).toContain('if [ "$result" = "skipped" ]')
    for (const platform of ['linux', 'windows', 'macos']) {
      expect(verification).toContain(`${platform}) result="$${platform.toUpperCase()}_RESULT"`)
    }
  })
})

describe('nightly E2E JUnit fallback', () => {
  it('records the desktop-ci exit status with every lane envelope diagnostic', () => {
    expect(workflow.match(/extract-artifacts\.sh "\$output" "\$RUNNER_TEMP\/[a-z0-9-]+" "\$run_status"/g)).toHaveLength(3)
  })

  it.skipIf(process.platform === 'win32')('covers Linux and macOS setup failures with valid artifact envelopes', () => {
    for (const suite of ['installed-linux', 'installed-macos']) {
      const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
      fs.writeFileSync(path.join(artifacts, 'diagnostics.log'), 'retained')
      runReportFallback(artifacts, suite, 1, 0)
      const report = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
      const xml = new DOMParser().parseFromString(report, 'application/xml')
      expect(xml.querySelector('parsererror')).toBeNull()
      expect(xml.querySelector('failure')).not.toBeNull()
      expect(report).toContain(`<testsuite name="${suite}" tests="1" failures="1">`)
      expect(report).toContain('<failure message="desktop-ci failed before producing a JUnit report"/>')
      expect(fs.existsSync(path.join(artifacts, 'diagnostics.log'))).toBe(true)
    }
  })

  it.skipIf(process.platform === 'win32').each([0, 1])('uses the cause with extraction status %s', (extractStatus) => {
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
    try {
      const cause = 'lookup <failed> & "stopped" at \'nightly\''
      fs.writeFileSync(path.join(artifacts, 'runner-failure.txt'), `\ufeff${cause}\r\n`)
      fs.writeFileSync(path.join(artifacts, 'envelope-reason.txt'), 'secondary cause')
      runReportFallback(artifacts, 'installed-windows', 1, extractStatus)
      const report = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
      const xml = new DOMParser().parseFromString(report, 'application/xml')
      expect(xml.querySelector('parsererror')).toBeNull()
      expect(xml.querySelector('failure').getAttribute('message')).toBe(cause)
      expect(report).toContain('&lt;failed&gt; &amp; &quot;stopped&quot; at &apos;nightly&apos;')
    } finally {
      fs.rmSync(artifacts, { recursive: true, force: true })
    }
  })

  it.skipIf(process.platform === 'win32').each([999, 1000, 1001])('caps a %s-character cause without splitting XML entities', (length) => {
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
    try {
      const cause = '界'.repeat(length - 1) + '&'
      fs.writeFileSync(path.join(artifacts, 'runner-failure.txt'), cause)
      runReportFallback(artifacts, 'installed-macos', 1, 0)
      const xml = new DOMParser().parseFromString(fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8'), 'application/xml')
      expect(xml.querySelector('parsererror')).toBeNull()
      expect(xml.querySelector('failure').getAttribute('message')).toBe(cause.slice(0, 1000))
    } finally {
      fs.rmSync(artifacts, { recursive: true, force: true })
    }
  })

  it.skipIf(process.platform === 'win32').each([undefined, '', '\r\n \t'])('uses the generic envelope failure without a cause: %s', (cause) => {
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
    try {
      if (cause !== undefined) fs.writeFileSync(path.join(artifacts, 'runner-failure.txt'), cause)
      runReportFallback(artifacts, 'installed-windows', 1, 1)
      expect(fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8'))
        .toContain('<failure message="desktop-ci did not return a valid artifact envelope"/>')
    } finally {
      fs.rmSync(artifacts, { recursive: true, force: true })
    }
  })

  it.skipIf(process.platform === 'win32')('retains an existing JUnit report instead of replacing it', () => {
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
    const existing = path.join(artifacts, 'junit-results.xml')
    fs.writeFileSync(existing, '<testsuites/>')
    runReportFallback(artifacts, 'installed-linux', 1, 0)
    expect(fs.readFileSync(existing, 'utf8')).toBe('<testsuites/>')
    expect(fs.existsSync(path.join(artifacts, 'junit-infrastructure.xml'))).toBe(false)
  })

  it.skipIf(process.platform === 'win32')('creates the macOS smoke report required on a successful run', () => {
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
    runReportFallback(artifacts, 'installed-macos', 0, 0, true)
    const report = fs.readFileSync(path.join(artifacts, 'junit-smoke.xml'), 'utf8')
    const xml = new DOMParser().parseFromString(report, 'application/xml')
    expect(xml.querySelector('parsererror')).toBeNull()
    expect(xml.querySelector('testsuites').getAttribute('failures')).toBe('0')
    expect(xml.querySelector('testcase').getAttribute('name')).toBe('installed application smoke')
    expect(xml.querySelector('failure')).toBeNull()
  })
})

describe('nightly failure reporting', () => {
  it('files no GitHub issue: the failure envelope is the record and Plane holds the tickets', () => {
    expect(workflow).not.toContain('report-e2e-failure')
    expect(workflow).not.toContain('issues: write')
    expect(workflow).not.toContain('github.rest.issues')
    expect(workflow).not.toContain('[nightly-e2e]')
  })
})

import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

const workflow = fs.readFileSync('.github/workflows/nightly.yml', 'utf8')
const ensureJunitReport = 'test/e2e/support/ensure-junit-report.sh'

const runReportFallback = (directory, suite, runStatus, extractStatus, createSuccessReport = false) => {
  const result = spawnSync('bash', [ensureJunitReport, directory, suite, String(runStatus), String(extractStatus), createSuccessReport ? '1' : '0'], { encoding: 'utf8' })
  expect(result.status, result.stderr).toBe(0)
}
const publish = workflow.slice(workflow.indexOf('  publish:'), workflow.indexOf('  linux-e2e:'))
const linuxE2e = workflow.slice(workflow.indexOf('  linux-e2e:'), workflow.indexOf('  windows-e2e:'))
const jobCondition = linuxE2e.match(/    if: >-\n((?:      .+\n)+)/)[1].trim().replace(/\n\s*/g, ' ')
const conditionResult = ({ eventName, platform, build, publish }) => {
  const expression = jobCondition
    .replace('always()', 'true')
    .replaceAll('needs.build.result', JSON.stringify(build))
    .replaceAll('needs.publish.result', JSON.stringify(publish))
    .replaceAll('github.event_name', JSON.stringify(eventName))
    .replaceAll('github.event.inputs.platform', JSON.stringify(platform))
  return Function(`"use strict"; return (${expression})`)()
}

describe('nightly Linux E2E workflow', () => {
  it('finalizes with contents write without PATCHing target_commitish', () => {
    expect(publish).toContain('permissions:\n      contents: write')
    expect(publish).not.toContain('target_commitish: sha')
    expect(publish.indexOf('github.rest.git.updateRef')).toBeLessThan(publish.indexOf('github.rest.repos.updateRelease'))
  })

  it('runs after a successful full-nightly publish', () => {
    expect(conditionResult({ eventName: 'schedule', platform: '', build: 'success', publish: 'success' })).toBe(true)
  })

  it('runs a Linux-only dispatch after its package build and finalizes the release identity', () => {
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'linux', build: 'success', publish: 'skipped' })).toBe(true)
    expect(linuxE2e).toContain("if: github.event_name == 'workflow_dispatch' && github.event.inputs.platform == 'linux'")
    expect(linuxE2e).toContain('ref: "tags/nightly"')
    expect(linuxE2e).toContain('`nightly-${sha}-linux-muniment.deb`')
  })

  it('does not run for another targeted platform or an unsuccessful package build', () => {
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'windows', build: 'success', publish: 'skipped' })).toBe(false)
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'linux', build: 'failure', publish: 'skipped' })).toBe(false)
  })

  it('uploads only generated JUnit XML under the stable report name', () => {
    const reportStep = linuxE2e.slice(linuxE2e.indexOf('      - name: Upload stable JUnit report'), linuxE2e.indexOf('      - name: Preserve E2E result'))
    expect(linuxE2e).toContain('ensure-junit-report.sh')
    expect(reportStep).toContain('if: always()')
    expect(reportStep).toContain('name: linux-e2e-report')
    expect(reportStep).toContain('path: ${{ runner.temp }}/muniment-e2e-artifacts/junit-*.xml')
    expect(reportStep).not.toContain('path: ${{ runner.temp }}/muniment-e2e-artifacts\n')
  })

  it('requests the desktop-ci artifact collector for the guest-published report', () => {
    const runStep = linuxE2e.slice(linuxE2e.indexOf('      - name: Run installed Linux sign-in via desktop-ci'), linuxE2e.indexOf('      - name: Upload successful diagnostics'))
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

  it('uses the SSH key provided by the self-hosted runner for every E2E lane', () => {
    expect(workflow).not.toContain('DESKTOP_CI_SSH_KEY: ${{ secrets.DESKTOP_CI_SSH_KEY }}')
    expect(workflow.match(/printf '%s' "\$DESKTOP_CI_SSH_KEY"/g)).toHaveLength(4)
  })
})

describe('nightly Windows E2E workflow', () => {
  const start = workflow.indexOf('  windows-e2e:')
  const windowsE2e = workflow.slice(start, workflow.indexOf('  macos-e2e:', start))

  it('uploads only generated JUnit XML under the stable report name', () => {
    const reportStep = windowsE2e.slice(windowsE2e.indexOf('      - name: Upload stable JUnit report'), windowsE2e.indexOf('      - name: Preserve E2E result'))
    expect(reportStep).toContain('if: always()')
    expect(reportStep).toContain('name: windows-e2e-report')
    expect(reportStep).toContain('path: ${{ runner.temp }}/muniment-windows-e2e-artifacts/junit-*.xml')
    expect(reportStep).not.toContain('path: ${{ runner.temp }}/muniment-windows-e2e-artifacts\n')
  })

  it('publishes a parseable infrastructure failure when an extracted failure has no JUnit', () => {
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

  it('validates the pinned SHA and sends no sign-in fixture credentials', () => {
    expect(macosE2e).toContain('^[0-9a-f]{40}$')
    expect(macosE2e).not.toContain('DESKTOP_E2E_USERNAME')
    expect(macosE2e).not.toContain('DESKTOP_E2E_PASSWORD')
    expect(macosE2e).not.toContain('MUNIMENT_E2E_USERNAME')
    expect(macosE2e).not.toContain('MUNIMENT_E2E_PASSWORD')
  })

  it('publishes diagnostics and a stable JUnit report with the required retention', () => {
    expect(macosE2e).toContain('name: macos-e2e-${{ needs.prepare.outputs.source_sha }}-success')
    expect(macosE2e).toMatch(/name: macos-e2e-\$\{\{ needs\.prepare\.outputs\.source_sha \}\}-success[\s\S]*?retention-days: 7/)
    expect(macosE2e).toMatch(/name: macos-e2e-\$\{\{ needs\.prepare\.outputs\.source_sha \}\}-failure[\s\S]*?retention-days: 30/)
    expect(macosE2e).toContain('ensure-junit-report.sh')
    expect(macosE2e).toContain('name: macos-e2e-report')
    expect(macosE2e).toContain('path: ${{ runner.temp }}/muniment-macos-e2e-artifacts/junit-*.xml')
  })
})

describe('nightly E2E JUnit fallback', () => {
  it('records the desktop-ci exit status with every lane envelope diagnostic', () => {
    expect(workflow.match(/extract-artifacts\.sh "\$output" "\$RUNNER_TEMP\/[a-z0-9-]+" "\$run_status"/g)).toHaveLength(3)
  })

  it('covers Linux and macOS setup failures with valid artifact envelopes', () => {
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

  it('retains an existing JUnit report instead of replacing it', () => {
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-junit-'))
    const existing = path.join(artifacts, 'junit-results.xml')
    fs.writeFileSync(existing, '<testsuites/>')
    runReportFallback(artifacts, 'installed-linux', 1, 0)
    expect(fs.readFileSync(existing, 'utf8')).toBe('<testsuites/>')
    expect(fs.existsSync(path.join(artifacts, 'junit-infrastructure.xml'))).toBe(false)
  })

  it('creates the macOS smoke report required on a successful run', () => {
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

describe('nightly installed-E2E failure reporting', () => {
  const start = workflow.indexOf('  report-e2e-failure:')
  const report = workflow.slice(start)
  const reportCondition = report.match(/    if: >-\n((?:      .+\n)+)/)[1].trim().replace(/\n\s*/g, ' ')
  const shouldReport = ({ linux, windows, macos }) => Function(
    `"use strict"; return (${reportCondition
      .replace('always()', 'true')
      .replaceAll('needs.linux-e2e.result', JSON.stringify(linux))
      .replaceAll('needs.windows-e2e.result', JSON.stringify(windows))
      .replaceAll('needs.macos-e2e.result', JSON.stringify(macos))})`,
  )()

  it('waits for every installed-E2E lane and runs only when one failed', () => {
    expect(report).toContain('needs: [prepare, linux-e2e, windows-e2e, macos-e2e]')
    expect(reportCondition).toContain('always()')
    expect(shouldReport({ linux: 'success', windows: 'success', macos: 'success' })).toBe(false)
    expect(shouldReport({ linux: 'skipped', windows: 'skipped', macos: 'skipped' })).toBe(false)
    expect(shouldReport({ linux: 'success', windows: 'skipped', macos: 'success' })).toBe(false)
    expect(shouldReport({ linux: 'failure', windows: 'skipped', macos: 'skipped' })).toBe(true)
    expect(shouldReport({ linux: 'success', windows: 'failure', macos: 'success' })).toBe(true)
    expect(shouldReport({ linux: 'success', windows: 'success', macos: 'failure' })).toBe(true)
  })

  it('uses only safe workflow metadata in the issue body', () => {
    expect(report).toContain('Pinned source SHA:')
    expect(report).toContain('Failing platforms:')
    expect(report).toContain('/actions/runs/${context.runId}')
    expect(report).not.toContain('FIXTURE_')
    expect(report).not.toContain('MUNIMENT_E2E_USERNAME')
    expect(report).not.toContain('MUNIMENT_E2E_PASSWORD')
    expect(report).not.toContain('secrets.')
    expect(report).not.toContain('readFile')
  })

  it('deduplicates open automation issues by the full pinned SHA', () => {
    expect(report).toContain('const title = `[nightly-e2e] Installed test failure for ${sha}`')
    expect(report).toContain('state: "open"')
    expect(report).toContain('issue.title === title')
    expect(report).toContain('github.rest.issues.createComment')
    expect(report).toContain('github.rest.issues.create({ ...context.repo, title, body })')
    expect(report).toContain('/^[0-9a-f]{40}$/')
  })

  it('grants only job-scoped issue access and does not hide API failures', () => {
    expect(report).toContain('permissions:\n      issues: write')
    expect(report).not.toContain('contents:')
    expect(report).not.toContain('continue-on-error')
    expect(report).not.toContain('try {')
    expect(report).not.toContain('catch (')
  })
})

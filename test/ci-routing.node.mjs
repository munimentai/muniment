import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { spawnSync } from 'node:child_process'

const workflows = Object.fromEntries(readdirSync('.github/workflows')
  .filter(name => name.endsWith('.yml'))
  .map(name => [name, readFileSync(`.github/workflows/${name}`, 'utf8')]))
const jobs = text => Object.fromEntries([...text.slice(text.indexOf('\njobs:\n'))
  .matchAll(/^  ([\w-]+):\n([\s\S]*?)(?=^  [\w-]+:\n|$(?![\s\S]))/gm)]
  .map(([, name, body]) => [name, body]))

const assignments = {
  'ci.yml': {
    checks: 'muniment-checks', changes: 'muniment-checks', smoke: 'self-hosted',
    'attach-fixtures-current': 'self-hosted', 'code-diff-fixtures-current': 'self-hosted',
    'desktop-compile': 'self-hosted',
  },
  'secret-scan.yml': { gitleaks: 'muniment-checks' },
  'artifact-store.yml': { checks: 'muniment-checks', upload: 'macos-15', collect: 'muniment-checks' },
  'nightly.yml': {
    prepare: 'muniment-checks', build: 'muniment-release', publish: 'muniment-checks',
    'linux-e2e': 'self-hosted', 'windows-e2e': 'self-hosted', 'macos-e2e': 'self-hosted',
    'verify-requested-e2e': 'muniment-checks', proof: 'muniment-checks',
  },
  'subscriptions.yml': {
    'validate-platform': 'muniment-checks', linux: 'muniment-release', windows: 'muniment-release',
    'macos-arm64': 'macos-15', 'macos-x64': 'muniment-release', collect: 'muniment-checks',
  },
  'release.yml': { promote: 'muniment-release', 'draft-winget-pr': 'muniment-release' },
}

test('Every workflow keeps bounded checks separate from native and resource-heavy work.', () => {
  assert.deepEqual(Object.keys(workflows).sort(), Object.keys(assignments).sort())
  for (const [workflow, expected] of Object.entries(assignments)) {
    const actual = Object.fromEntries(Object.entries(jobs(workflows[workflow]))
      .flatMap(([name, body]) => {
        const runner = body.match(/^    runs-on: (.+)$/m)?.[1]
        return runner ? [[name, runner]] : []
      }))
    assert.deepEqual(actual, expected, workflow)
    for (const [name, runner] of Object.entries(actual)) {
      if (runner !== 'muniment-checks') continue
      assert.doesNotMatch(jobs(workflows[workflow])[name], /DESKTOP_CI_SSH_KEY|sudo desktop-ci|npm ci|cargo (?:test|build|clippy)/)
    }
  }
})

test('Only stale checks for the same PR share cancellable groups.', () => {
  const scopes = [
    ['ci', workflows['ci.yml']],
    ['secret-scan', workflows['secret-scan.yml']],
    ['artifact-store-checks', jobs(workflows['artifact-store.yml']).checks],
  ]
  const groups = new Set()
  for (const [prefix, scope] of scopes) {
    const group = scope.match(/group: (.+)/)[1]
    assert.equal(group, `${prefix}-\${{ github.event_name }}-\${{ github.event.pull_request.number || github.run_id }}`)
    assert.match(scope, /cancel-in-progress: \$\{\{ github.event_name == 'pull_request' \}\}/)
    const resolve = (pr, run, event = pr ? 'pull_request' : 'push') => group
      .replace('${{ github.event_name }}', event)
      .replace('${{ github.event.pull_request.number || github.run_id }}', String(pr || run))
    assert.equal(resolve(12, 100), resolve(12, 101))
    assert.notEqual(resolve(12, 100), resolve(13, 101))
    assert.notEqual(resolve(undefined, 100), resolve(undefined, 101))
    assert.notEqual(resolve(12, 100), resolve(undefined, 12))
    assert.notEqual(resolve(undefined, 100), resolve(undefined, 100, 'workflow_dispatch'))
    assert.equal(groups.has(resolve(12, 100)), false)
    groups.add(resolve(12, 100))
  }
  for (const name of ['nightly.yml', 'subscriptions.yml', 'release.yml']) {
    assert.match(workflows[name], /cancel-in-progress: false/)
    assert.doesNotMatch(workflows[name], /cancel-in-progress: (?:true|\$)/)
  }
  assert.match(workflows['subscriptions.yml'], /group: subscription-acceptance-\$\{\{ inputs.source_sha \}\}-\$\{\{ github.sha \}\}-\$\{\{ inputs.platform \}\}/)
})

test('Artifact collection stays local and the ARM upload probe keeps its native coverage.', () => {
  const artifact = jobs(workflows['artifact-store.yml'])
  assert.match(artifact.checks, /node --test test\/artifact-store.node.mjs test\/ci-routing.node.mjs/)
  assert.match(artifact.upload, /test "\$\(uname -m\)" = arm64/)
  assert.match(artifact.upload, /artifact-store-probe.mjs upload/)
  assert.match(artifact.collect, /needs: upload/)
  assert.match(artifact.collect, /artifact-store-probe.mjs collect/)
  assert.doesNotMatch(artifact.upload + artifact.collect, /concurrency:/)
})

test('The bounded CI job keeps the steering and copy checks with their reuse gates.', () => {
  const ci = jobs(workflows['ci.yml'])
  assert.match(ci.checks, /needs: changes/)
  assert.match(ci.checks, /name: Steering check\n        if: needs.changes.outputs.reuse != 'true'\n        run: scripts\/check-steering.sh ./)
  assert.match(ci.checks, /name: UI copy law\n        if: needs.changes.outputs.reuse != 'true' && \(needs.changes.outputs.desktop == 'true' \|\| github.event_name != 'pull_request'\)\n        run: node test\/ui-copy-lint.mjs src src-tauri browser-control/)
  assert.doesNotMatch(ci.checks, /\bnpm\b/)
  const copyCommand = ci.checks.match(/run: (node test\/ui-copy-lint.mjs [^\n]+)/)[1]
  const copyRun = spawnSync(process.execPath, copyCommand.split(' ').slice(1), {
    env: { ...process.env, PATH: '' }, timeout: 30000, encoding: 'utf8',
  })
  assert.equal(copyRun.status, 0, copyRun.stderr || copyRun.stdout)
  assert.doesNotMatch(ci.smoke, /name: Steering check|name: UI copy law/)
  assert.match(ci.smoke, /if: \$\{\{ !cancelled\(\) && needs.changes.result == 'success' \}\}/)
  assert.match(ci.smoke, /needs: \[changes, checks\]/)
  assert.match(ci.smoke, /CHECKS_RESULT: \$\{\{ needs.checks.result \}\}/)
  assert.match(ci.smoke, /run: test "\$CHECKS_RESULT" = success/)
  const command = ci.smoke.match(/run: (test "\$CHECKS_RESULT" = success)/)[1]
  for (const result of ['success', 'failure', 'skipped', 'cancelled', '', 'success failure']) {
    const run = spawnSync('bash', ['-e', '-c', command], {
      env: { ...process.env, CHECKS_RESULT: result }, timeout: 5000,
    })
    assert.equal(run.status, result === 'success' ? 0 : 1)
  }
})

test('PR proof writers and main readers bind the same off-tree configuration.', () => {
  for (const name of ['ci.yml', 'secret-scan.yml']) {
    assert.match(workflows[name], /CI_PROOF_CONFIGURATION: \$\{\{ toJSON\(vars\) \}\}/)
    assert.match(workflows[name], /reusePullRequest\(/)
    assert.match(workflows[name], /writeProof\(/)
  }
})

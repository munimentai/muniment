import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
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
    prepare: 'muniment-checks', build: 'self-hosted', publish: 'muniment-checks',
    'linux-e2e': 'self-hosted', 'windows-e2e': 'self-hosted', 'macos-e2e': 'self-hosted',
    'verify-requested-e2e': 'muniment-checks',
  },
  'subscriptions.yml': {
    'validate-platform': 'muniment-checks', linux: 'muniment-release', windows: 'muniment-release',
    'macos-arm64': 'macos-15', 'macos-x64': 'muniment-release', collect: 'muniment-checks',
  },
  'release.yml': {
    promote: 'self-hosted', 'draft-winget-pr': 'self-hosted',
    'server-release': '${{ vars.CI_LINUX_RUNNER }}', 'runtime-image': 'image-smoke',
  },
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
  assert.match(ci.checks, /name: Private address check\n        run: \|\n          scripts\/check-private-addresses.sh\n          python3 -B scripts\/test_private_addresses.py\n/)
  assert.match(ci.checks, /name: UI copy law\n        if: needs.changes.outputs.reuse != 'true' && \(needs.changes.outputs.desktop == 'true'\)\n        run: node test\/ui-copy-lint.mjs src src-tauri browser-control/)
  assert.match(ci.checks, /name: Version check\n        run: \|\n          python3 -B scripts\/version.py check\n          python3 -B scripts\/test_version.py\n/)
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

test('Core runs on Forgejo only, in core/, off the desktop runners, and tags no release.', () => {
  const core = readFileSync('.forgejo/workflows/core.yml', 'utf8')
  const pins = readFileSync('.forgejo/workflows/core-pins-update.yml', 'utf8')
  assert.equal(workflows['core.yml'], undefined)
  for (const text of [core, pins]) {
    assert.match(text, /^defaults:\n  run:\n    working-directory: core\n/m)
    assert.doesNotMatch(text, /\.forgejo\/[\w-]+\.sh|runs-on: (?:\[?self-hosted|muniment-checks)/)
  }
  for (const event of ['push', 'pull_request']) {
    const filter = core.match(new RegExp(`^  ${event}:\\n(?:    branches: .*\\n)?    paths:\\n((?:      - .*\\n)+)`, 'm'))[1]
    assert.deepEqual(filter.trim().split('\n').map(line => line.trim()),
      ['- "core/**"', '- ".cargo/**"', '- "src-tauri/third-party/**"', '- ".forgejo/workflows/core*.yml"',
        '- "scripts/version.py"', '- "src-tauri/Cargo.toml"', '- "src-tauri/Cargo.lock"'])
  }
  const runners = Object.fromEntries(Object.entries(jobs(core))
    .map(([name, body]) => [name, body.match(/^    runs-on: (.+)$/m)[1]]))
  assert.deepEqual(runners, {
    checks: '${{ vars.CI_LINUX_RUNNER }}', 'router-server': '${{ vars.CI_LINUX_RUNNER }}',
    'router-image': 'image-smoke', 'runtime-image': 'image-smoke', macos: '${{ vars.CI_LINUX_RUNNER }}',
    windows: '${{ vars.CI_LINUX_RUNNER }}',
  })
  // Core releases with the desktop under one vX.Y.Z tag, which only release.yml creates.
  assert.doesNotMatch(core, /core-v|release\.py --publish|gh release/)
  assert.doesNotMatch(pins, /core-v|GH_RELEASE_TOKEN|MUNIMENT_RELEASE_SIGNING_KEY/)
  assert.match(core, /python3 -B \.\.\/scripts\/version\.py check/)
  assert.match(core, /cargo metadata --locked --format-version 1 --manifest-path \.\.\/src-tauri\/Cargo.toml/)
  assert.match(pins, /cargo check --manifest-path \.\.\/src-tauri\/Cargo.toml --locked -p muniment-runtime/)
  for (const vm of [jobs(core).macos, jobs(core).windows, jobs(pins).update]) {
    assert.match(vm, /desktop-ci.sh (?:macos|windows)[\s\\]*\n?\s*"cd core &&/)
  }
})

test('One vX.Y.Z release carries the desktop, the server files and the runtime image.', () => {
  const release = jobs(workflows['release.yml'])
  assert.match(release['server-release'], /needs: promote\n/)
  assert.match(release['server-release'], /"v\$source" != "\$VERSION"/)
  assert.match(release['server-release'], /gh release download "\$VERSION" --repo munimentai\/muniment --pattern SHA256SUMS/)
  assert.match(release['server-release'], /deploy\/runtime\/release-assets.sh "\$\{VERSION#v\}" dist "\$\{sums\[@\]\}"/)
  const uploads = [...release['server-release'].matchAll(/gh release upload "\$VERSION" .*/g)].map(([line]) => line)
  assert.equal(uploads.length, 2)
  assert.match(uploads[0], / dist\/SHA256SUMS dist\/SHA256SUMS.minisig$/)
  assert.match(uploads[1], / pins\/pins.toml pins\/packages.bun.lock$/)
  assert.match(release['runtime-image'], /needs: server-release\n/)
  assert.match(release['runtime-image'], /deploy\/runtime\/publish.sh "\$VERSION" "\$SOURCE_SHA"/)
  assert.doesNotMatch(workflows['release.yml'], /core-v|--latest=false/)
})

const routing = await import('./ci-workflow.mjs')
const coreSupportPaths = [
  'core/docs/core-boundary.md', 'core/docs/tool-presentation.md', 'core/deploy/runtime/Containerfile',
  'core/scripts/check-core-boundary.sh', 'core/scripts/check-core-boundary.py', 'core/scripts/release.py',
  'core/ci/desktop-ci.sh', 'core/ci/postgres.sh', 'core/ci/rust-toolchain.sh', 'core/ci/server-cross-build.sh',
  'core/ci/zig-toolchain.sh', 'core/test_support/socket_path.rs', 'core/Cargo.lock',
  '.forgejo/workflows/core.yml', '.forgejo/workflows/core-pins-update.yml',
]
const pushScenarios = [
  ['a pull request', {}],
  ['a job-token push with a known base', { event: 'push', base: 'a'.repeat(40) }],
]
const workspaceTest = ({ command }) => /--workspace\b|--all\b/.test(command) || !/(?:^|\s)(?:-p|--package)\b/.test(command)

test('The routed support paths cover every core CI helper.', () => {
  for (const name of readdirSync('core/ci')) assert.ok(coreSupportPaths.includes(`core/ci/${name}`), name)
  for (const path of coreSupportPaths) assert.ok(existsSync(path), path)
})

for (const file of routing.mirrors) {
  test(`${file}: a core boundary inventory change runs the boundary checker, not the Markdown shortcut.`, () => {
    for (const [, options] of pushScenarios) {
      const { outputs, scheduled } = routing.route(file, ['core/docs/core-boundary.md'], { ...options, reuse: 'false' })
      assert.equal(outputs.docs_only, 'false')
      assert.equal(outputs.desktop, 'false')
      assert.equal(outputs.installer, 'false')
      const [boundary] = routing.boundaryChecks(scheduled)
      assert.ok(boundary, 'Public core boundary is scheduled')
      assert.equal(boundary.name, 'Public core boundary')
      assert.match(boundary.run, /scripts\/check-core-boundary\.sh/)
      assert.equal(routing.boundaryChecks(routing.route(file, ['core/docs/core-boundary.md'], { ...options, reuse: 'true' }).scheduled).length, 0)
    }
    // The scheduled checker reads the inventory this path names.
    assert.match(readFileSync('scripts/check-core-boundary.sh', 'utf8'), /core\/scripts\/check-core-boundary\.sh/)
    assert.match(readFileSync('core/scripts/check-core-boundary.sh', 'utf8'), /scripts\/check-core-boundary\.py/)
    assert.match(readFileSync('core/scripts/check-core-boundary.py', 'utf8'), /Path\("docs\/core-boundary\.md"\)/)
  })

  test(`${file}: a core lockfile change runs a locked core workspace test.`, () => {
    for (const [scenario, options] of pushScenarios) {
      const { outputs, scheduled } = routing.route(file, ['core/Cargo.lock'], { ...options, reuse: 'false' })
      assert.equal(outputs.docs_only, 'false', scenario)
      const tests = routing.coreWorkspaceTests(scheduled).filter(workspaceTest)
      assert.ok(tests.length > 0, `${scenario}: a cargo test resolves core/Cargo.lock with --locked`)
      for (const { lockfile, locked } of tests) {
        assert.equal(lockfile, 'core/Cargo.lock')
        assert.equal(locked, true)
      }
    }
  })

  for (const path of coreSupportPaths) {
    test(`${file}: ${path} alone runs the core boundary and workspace tests without the desktop matrix.`, () => {
      for (const [scenario, options] of pushScenarios) {
        const { outputs, scheduled } = routing.route(file, [path], { ...options, reuse: 'false' })
        assert.equal(outputs.docs_only, 'false', scenario)
        assert.equal(outputs.desktop, 'false', scenario)
        assert.equal(outputs.installer, 'false', scenario)
        assert.equal(outputs.release_only, 'false', scenario)
        assert.ok(routing.boundaryChecks(scheduled).length > 0, `${scenario}: boundary check`)
        assert.ok(routing.coreWorkspaceTests(scheduled).filter(workspaceTest).length > 0, `${scenario}: core workspace test`)
        assert.deepEqual(routing.desktopPlatforms(scheduled).filter(platform => platform !== 'linux'), [], scenario)
      }
    })
  }

  test(`${file}: core and unrelated Markdown stay documentation-only.`, () => {
    for (const paths of [['core/README.md'], ['README.md'], ['docs/core-boundary.md'], ['README.md', 'core/README.md']]) {
      const { outputs, scheduled } = routing.route(file, paths, { reuse: 'false' })
      assert.deepEqual([outputs.docs_only, outputs.desktop, outputs.installer, outputs.core_shared, outputs.companion],
        ['true', 'false', 'false', 'false', 'false'], paths.join(' '))
      assert.deepEqual(routing.coreWorkspaceTests(scheduled), [], paths.join(' '))
      assert.deepEqual(routing.desktopPlatforms(scheduled), [], paths.join(' '))
    }
  })

  test(`${file}: mixed, fixture, installer and fallback routes keep their lanes.`, () => {
    for (const paths of [['src/App.svelte', 'core/Cargo.lock'], ['core/docs/core-boundary.md', 'src/App.svelte']]) {
      const { outputs, scheduled } = routing.route(file, paths)
      assert.equal(outputs.desktop, 'true')
      assert.deepEqual(routing.desktopPlatforms(scheduled), ['linux', 'macos', 'windows'])
      assert.ok(routing.coreWorkspaceTests(scheduled).filter(workspaceTest).length > 0, paths.join(' '))
    }
    const shared = routing.route(file, ['core/crates/core/src/lib.rs'])
    assert.equal(shared.outputs.core_shared, 'true')
    assert.deepEqual(routing.desktopPlatforms(shared.scheduled), ['linux'])
    const jobsFor = paths => new Set(routing.route(file, paths).scheduled.map(step => step.job))
    assert.ok(jobsFor(['core/protocol-fixtures/muniment-attach/1/hello.json']).has('attach-fixtures-current'))
    assert.ok(!jobsFor(['core/protocol-fixtures/muniment-attach/1/hello.json']).has('code-diff-fixtures-current'))
    assert.ok(jobsFor(['core/protocol-fixtures/code-diff/1/basic.json']).has('code-diff-fixtures-current'))
    assert.ok(!jobsFor(['core/Cargo.lock']).has('attach-fixtures-current'))
    assert.equal(routing.route(file, ['.github/build-linux.sh']).outputs.installer, 'true')
    assert.equal(routing.route(file, ['core/Cargo.lock']).outputs.installer, 'false')
    for (const options of [
      { event: 'push', base: '0'.repeat(40) }, { event: 'push', base: '' },
      { event: 'push', base: 'b'.repeat(40), knownBase: false }, { event: 'workflow_dispatch' },
    ]) {
      const { outputs, scheduled } = routing.route(file, ['core/README.md'], options)
      assert.deepEqual([outputs.desktop, outputs.installer, outputs.core_shared, outputs.attach_fixtures, outputs.code_diff_fixtures],
        ['true', 'true', 'true', 'true', 'true'], JSON.stringify(options))
      assert.deepEqual(routing.desktopPlatforms(scheduled), ['linux', 'macos', 'windows'])
      assert.ok(routing.boundaryChecks(scheduled).length > 0)
      assert.ok(routing.coreWorkspaceTests(scheduled).filter(workspaceTest).length > 0, JSON.stringify(options))
    }
  })

  test(`${file}: a reused tree schedules no boundary, core test or desktop work.`, () => {
    const { scheduled } = routing.route(file, ['core/Cargo.lock', 'src/App.svelte'], { event: 'push', reuse: 'true' })
    assert.deepEqual(routing.boundaryChecks(scheduled), [])
    assert.deepEqual(routing.cargoTests(scheduled), [])
    assert.deepEqual(routing.desktopPlatforms(scheduled), [])
  })
}

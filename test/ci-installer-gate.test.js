import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { execFileSync } from 'node:child_process'
import path from 'node:path'
import { tmpdir } from 'node:os'

const root = process.cwd()
const workflow = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8')
const job = (name, next) => workflow.slice(workflow.indexOf(`  ${name}:\n`), workflow.indexOf(`  ${next}:\n`))
const changes = job('changes', 'smoke')
const smoke = job('smoke', 'attach-fixtures-current')
const desktopCompile = workflow.slice(workflow.indexOf('  desktop-compile:\n'))

// The paths the nightly cannot prove one merge later without a red run.
const installerPaths = [
  'src-tauri/tauri*.conf.json',
  'src-tauri/windows/*',
  'src-tauri/packaging/*',
  'src-tauri/capabilities/*',
  '.github/build-*.mjs',
  '.github/build-*.sh',
  '.github/lib/*',
  '.github/upload-nightly-assets.mjs',
  '.github/workflows/ci.yml',
  'test/e2e/*',
  'test/windows-installers.ps1',
]

function classify(paths, event = 'pull_request') {
  const script = changes.split('        run: |\n')[1].replace(/^ {10}/gm, '')
  const output = execFileSync('bash', ['-eu', '-c', `
    git() { printf '%s' "$CHANGED_PATHS"; }
    GITHUB_OUTPUT=$(mktemp)
    trap 'rm -f "$GITHUB_OUTPUT"' EXIT
    ${script}
    cat "$GITHUB_OUTPUT"
  `], {
    encoding: 'utf8',
    timeout: 10000,
    env: {
      ...process.env,
      GITHUB_EVENT_NAME: event,
      BASE_SHA: 'base',
      HEAD_SHA: 'head',
      CHANGED_PATHS: paths.length ? `${paths.join('\n')}\n` : '',
    },
  })
  return Object.fromEntries(output.trim().split('\n').map((line) => line.split('=')))
}

describe('PR gate shape', () => {
  it('classifies the diff in its own job and exposes the installer flag', () => {
    expect(changes).toContain('installer: ${{ steps.changes.outputs.installer }}')
    expect(changes).toContain('desktop: ${{ steps.changes.outputs.desktop }}')
    expect(changes).toContain('echo "installer=$installer" >> "$GITHUB_OUTPUT"')
    const lines = changes.split('\n')
    const flag = lines.findLastIndex((line) => line.trim() === 'installer=true')
    expect(flag).toBeGreaterThan(0)
    let patternLine = flag - 1
    while (lines[patternLine].trim().startsWith('#')) patternLine -= 1
    const patterns = lines[patternLine].trim().replace(/\)$/, '').split('|')
    for (const p of installerPaths) expect(patterns).toContain(p)
  })

  it.each([
    '.github/build-linux.sh',
    '.github/build-windows-installers.mjs',
    '.github/build-macos-runtime.mjs',
    '.github/lib/windows-signing.mjs',
    '.github/lib/macos-signing.mjs',
    '.github/lib/nested/helper.mjs',
    'test/windows-installers.ps1',
  ])('enables the installer matrix for a change to %s alone', (file) => {
    expect(classify([file])).toMatchObject({ installer: 'true', desktop: 'true', docs_only: 'false' })
  })

  it.each([
    'AGENTS.md',
    'README.md',
    'SPEC.md',
    'ROADMAP.md',
    'DESIGN.md',
    'docs/decisions/0030-public-core-boundary.md',
  ])('skips the installer matrix for a change to %s alone', (file) => {
    expect(classify([file])).toMatchObject({ installer: 'false', desktop: 'false', docs_only: 'true' })
  })

  it('keeps the installer flag when other paths change in either order', () => {
    for (const paths of [
      ['test/windows-installers.ps1', 'README.md', 'src/App.svelte'],
      ['src/App.svelte', 'README.md', 'test/windows-installers.ps1'],
    ]) {
      expect(classify(paths)).toMatchObject({ installer: 'true', desktop: 'true', docs_only: 'false' })
    }
  })

  it('skips the installer matrix for an empty diff or an unrelated code change', () => {
    expect(classify([])).toMatchObject({ installer: 'false', desktop: 'false', docs_only: 'true' })
    expect(classify(['src/App.svelte'])).toMatchObject({ installer: 'false', desktop: 'true', docs_only: 'false' })
  })

  it('runs smoke and the preflights off the changes job, not off each other', () => {
    expect(smoke).toContain('    needs: changes\n')
    expect(smoke).not.toContain('Classify PR changes')
    expect(desktopCompile).toContain('    needs: changes\n')
    expect(desktopCompile).toContain("if: needs.changes.outputs.desktop == 'true'")
    expect(workflow).not.toContain('needs.smoke.outputs')
  })

  it('runs packaging after preflight within one platform VM', () => {
    expect(workflow).not.toContain('  desktop-build:\n')
    expect(desktopCompile).toContain('INSTALLER: ${{ needs.changes.outputs.installer }}')
    expect(desktopCompile).toContain('if [ "$INSTALLER" = true ]; then')
    expect(desktopCompile).toContain('cmd="$preflight_cmd && $cmd"')
    expect(desktopCompile).toContain('--build-timeout')
    expect(desktopCompile).toContain('platform: [linux, windows, macos]')
  })

  it.each([
    '.github/lib/release-promotion.mjs', '.github/lib/release-promotion.test.js',
    '.github/lib/update-feed.mjs', '.github/lib/update-feed.test.js',
    '.github/lib/winget-manifest.mjs', '.github/lib/winget-manifest.test.js',
    '.github/lib/winget-release.mjs',
  ])('runs release tests without native checks for %s', (file) => {
    expect(classify([file])).toMatchObject({ release_tooling: 'true', release_only: 'true',
      desktop: 'false', installer: 'false', docs_only: 'false' })
  })

  it('runs every lane for an unproven direct push', () => {
    expect(classify([], 'push')).toMatchObject({ desktop: 'true', installer: 'true',
      companion: 'true', attach_fixtures: 'true', code_diff_fixtures: 'true', release_only: 'false' })
  })

  it('preserves native and documentation gates in mixed changes', () => {
    const release = '.github/lib/release-promotion.mjs'
    for (const file of ['src/App.svelte', '.github/lib/windows-signing.mjs', '.github/workflows/ci.yml']) {
      for (const paths of [[release, file], [file, release]]) {
        expect(classify(paths)).toMatchObject({ release_tooling: 'true', release_only: 'false', desktop: 'true' })
      }
    }
    expect(classify([release, 'README.md'])).toMatchObject({ release_only: 'false', release_tooling: 'true' })
    expect(classify([])).toMatchObject({ release_only: 'false', release_tooling: 'false' })
    expect(smoke).toContain("if: needs.changes.outputs.reuse != 'true' && (needs.changes.outputs.release_tooling == 'true' && needs.changes.outputs.desktop != 'true')")
    expect(smoke).toContain('- name: Rust toolchain (bootstrap if missing)\n        if: needs.changes.outputs.reuse != \'true\'\n        run: |')
    expect(smoke).toContain("- name: Public core boundary\n        if: needs.changes.outputs.reuse != 'true' && (needs.changes.outputs.release_only != 'true')")
  })
})

const remoteSteps = workflow.split('        run: |\n').slice(1)
  .map(block => block.split(/\n {0,8}\S/)[0].replace(/^ {10}/gm, ''))
  .filter(block => block.includes('sudo desktop-ci'))

it.each(remoteSteps.map((script, index) => [index, script]))('keeps clone credentials on stdin and preserves SSH failures in step %s', (_, script) => {
  expect(script).not.toContain('https://x-access-token:')
  for (const status of [0, 37]) {
    const directory = fs.mkdtempSync(path.join(tmpdir(), 'muniment-ci-auth-'))
    try {
      let actual = 0
      try {
        execFileSync('bash', ['-c', `
          ssh() {
            for arg in "$@"; do
              [[ $arg != *"$REPO_TOKEN"* ]] || exit 91
            done
            [[ "$*" == *--env-stdin* ]] || exit 92
            IFS= read -r credential
            [[ $credential == "GH_TOKEN=$REPO_TOKEN" ]] || exit 93
            return ${status}
          }
          ${script}
        `], { env: { ...process.env, REPO_TOKEN: 'fixture-only-secret', REPOSITORY: 'owner/repo',
          SOURCE_SHA: 'a'.repeat(40), REF: 'test-branch', PLATFORM: 'linux', INSTALLER: 'false', RUNNER_TEMP: directory, DESKTOP_CI_SSH_KEY: 'fixture-key',
          DESKTOP_CI_KNOWN_HOSTS: '10.1.10.10 ssh-ed25519 fixture' },
        stdio: 'pipe', timeout: 10000 })
      } catch (error) { actual = error.status }
      expect(actual).toBe(status)
    } finally { fs.rmSync(directory, { recursive: true, force: true }) }
  }
})

it('covers the fixture and native CI command steps', () => {
  expect(remoteSteps).toHaveLength(2)
})

it.each(['linux', 'windows', 'macos'])('uses one VM and gates packaging on preflight success for %s', (platform) => {
  const script = remoteSteps.find(block => block.includes('preflight_cmd=$cmd'))
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'muniment-ci-reuse-'))
  try {
    const capture = path.join(directory, 'remote-command')
    const env = { ...process.env, PLATFORM: platform, REPO_TOKEN: 'fixture-token',
      REPOSITORY: 'owner/repo', SOURCE_SHA: 'a'.repeat(40), REF: 'fixture-branch', RUNNER_TEMP: directory,
      DESKTOP_CI_SSH_KEY: 'fixture-key', DESKTOP_CI_KNOWN_HOSTS: '10.1.10.10 ssh-ed25519 fixture', CAPTURE: capture }
    const commands = {}
    for (const installer of ['false', 'true']) {
      fs.writeFileSync(capture, '')
      execFileSync('bash', ['-c', `
        ssh() { for arg in "$@"; do remote=$arg; done; printf '%s\\n' "$remote" >> "$CAPTURE"; cat >/dev/null; }
        ${script}
      `], { env: { ...env, INSTALLER: installer }, stdio: 'pipe', timeout: 10000 })
      const calls = fs.readFileSync(capture, 'utf8').trim().split('\n')
      expect(calls).toHaveLength(1)
      expect(calls[0]).toContain(`--build-timeout '${installer === 'true' ? 5400 : 3600}'`)
      commands[installer] = calls[0].match(/--cmd '([^']+)'/)?.[1]
      expect(commands[installer]).toBeTruthy()
    }
    expect(commands.true.startsWith(`${commands.false} && `)).toBe(true)
    if (platform === 'windows') {
      expect(commands.true.match(/npm ci/g)).toHaveLength(1)
      expect(commands.true).toContain('-File test/windows-installers.ps1')
    }
    const bin = path.join(directory, 'bin')
    fs.mkdirSync(bin)
    for (const command of ['git', 'cargo', 'npm', 'sudo', 'bash', 'node', 'rustup', 'clang', 'powershell.exe']) {
      const file = path.join(bin, command)
      fs.writeFileSync(file, `#!/bin/sh\nprintf '%s\\n' "$0 $*" >> "$COMMAND_LOG"\ncase "$0" in */cargo) [ "$FAIL_PREFLIGHT" = true ] && exit 29 ;; esac\nexit 0\n`)
      fs.chmodSync(file, 0o755)
    }
    fs.mkdirSync(path.join(directory, 'scripts'))
    fs.writeFileSync(path.join(directory, 'scripts/prepare-cef-macos.sh'), ':\n')
    for (const failure of ['false', 'true']) {
      const log = path.join(directory, 'commands')
      fs.writeFileSync(log, '')
      let status = 0
      try {
        execFileSync('/bin/bash', ['-c', commands.true], { cwd: directory, env: {
          ...process.env, PATH: `${bin}:${process.env.PATH}`, COMMAND_LOG: log, FAIL_PREFLIGHT: failure,
        }, stdio: 'pipe', timeout: 10000 })
      } catch (error) { status = error.status }
      expect(status).toBe(failure === 'true' ? 29 : 0)
      const executed = fs.readFileSync(log, 'utf8')
      const packaging = platform === 'windows' ? 'build-windows-installers.mjs'
        : platform === 'linux' ? 'build-linux.sh' : 'package-cef-macos.mjs'
      expect(executed.includes(packaging)).toBe(failure === 'false')
    }
  } finally { fs.rmSync(directory, { recursive: true, force: true }) }
})

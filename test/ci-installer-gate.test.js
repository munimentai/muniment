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
const desktopCompile = job('desktop-compile', 'desktop-build')
const desktopBuild = workflow.slice(workflow.indexOf('  desktop-build:\n'))

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

function classify(paths) {
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
    const flag = lines.findIndex((line) => line.trim() === 'installer=true')
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
    expect(desktopCompile).toContain("if: github.event_name == 'pull_request' && needs.changes.outputs.desktop == 'true'")
    expect(workflow).not.toContain('needs.smoke.outputs')
  })

  it('builds the three installers on a pull request only for installer paths', () => {
    expect(desktopBuild).toContain("if: github.event_name == 'pull_request' && needs.changes.outputs.desktop == 'true' && needs.changes.outputs.installer == 'true'")
    expect(desktopBuild).toContain('needs: [changes, desktop-compile]')
    expect(desktopBuild).toContain('platform: [linux, windows, macos]')
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
          REF: 'test-branch', PLATFORM: 'linux', RUNNER_TEMP: directory, DESKTOP_CI_SSH_KEY: 'fixture-key' },
        stdio: 'pipe', timeout: 10000 })
      } catch (error) { actual = error.status }
      expect(actual).toBe(status)
    } finally { fs.rmSync(directory, { recursive: true, force: true }) }
  }
})

it('covers all three remote CI command steps', () => {
  expect(remoteSteps).toHaveLength(3)
})

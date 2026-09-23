import { afterEach, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const temporary = []
afterEach(() => {
  for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true })
})

const fixture = () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-macos-config-'))
  temporary.push(directory)
  const loginHome = path.join(directory, 'login home')
  const loginState = path.join(loginHome, '.muniment')
  fs.mkdirSync(loginHome, { recursive: true })
  const shell = (script) => spawnSync('bash', ['-c', `
set -eu
source test/e2e/support/macos-spec-config.sh
${script}
`], {
    encoding: 'utf8', timeout: 10_000,
    env: { ...process.env, HOME: loginHome, LOGIN_HOME: loginHome, ROOT: directory, NODE: process.execPath },
  })
  return { directory, loginHome, loginState, shell }
}

describe('macOS local mode config', () => {
  it('keeps the desktop, the runtime and the harness on one state root', () => {
    const desktop = fs.readFileSync('src-tauri/src/local_mode.rs', 'utf8')
    const runtime = fs.readFileSync('src-tauri/runtime/src/directories.rs', 'utf8')
    const core = fs.readFileSync('src-tauri/core/src/state_root.rs', 'utf8')
    const resolver = desktop.slice(desktop.indexOf('fn config_directory'), desktop.indexOf('pub(crate) fn is_active'))
    expect(resolver).toContain('muniment_runtime::profile_directory()')
    expect(desktop).toMatch(/is_local_mode\(&config_directory\(\s*app,?\s*\)\?\)/)
    expect(desktop).toContain('set_local_mode(&config_directory(&app)?, true)')
    expect(desktop).toContain('set_local_mode(&config_directory(&app)?, false)')
    expect(runtime).toMatch(/pub fn profile_directory\(\)[\s\S]*?muniment_core::state_root::state_directory\(\)/)
    expect(runtime).toMatch(/pub fn config_directory\(\)[\s\S]*?profile_directory\(\)/)
    expect(core).toContain('pub const STATE_DIRECTORY_OVERRIDE: &str = "MUNIMENT_STATE_DIR";')
    expect(core).toContain('pub const STATE_DIRECTORY_NAME: &str = ".muniment";')
    // The earlier roots appear only in the adoption table, never in the resolver.
    const coreResolver = core.slice(core.indexOf('pub fn state_directory_from'), core.indexOf('pub fn agent_directory'))
    expect(coreResolver).not.toContain('Application Support')
    expect(coreResolver).not.toContain('.local/share')
  })

  it.skipIf(process.platform === 'win32').each(['absent', 'directory', 'symlink', 'dangling symlink'])(
    'links each spec home into the login home without the SSH environment and restores an original %s', (original) => {
      const { directory, loginState, shell } = fixture()
      const external = path.join(directory, 'external state')
      if (original !== 'absent') {
        if (original !== 'dangling symlink') {
          fs.mkdirSync(external, { mode: 0o700 })
          fs.writeFileSync(path.join(external, 'local-mode'), 'login marker', { mode: 0o600 })
        }
        if (original === 'directory') fs.renameSync(external, loginState)
        else fs.symlinkSync(external, loginState)
      }
      const probe = path.join(directory, 'probe.mjs')
      fs.writeFileSync(probe, `
import fs from 'node:fs'
import path from 'node:path'
if (process.env.MUNIMENT_STATE_DIR !== undefined) process.exit(2)
const state = path.join(process.env.HOME, '.muniment')
const marker = path.join(state, 'local-mode')
console.log(JSON.stringify({ state: fs.realpathSync(state), marker: fs.existsSync(marker) ? fs.readFileSync(marker, 'utf8') : null }))
`)
      const result = shell(`
restore_macos_spec_config
if set_macos_spec_config; then exit 2; fi
save_macos_spec_config
if save_macos_spec_config; then exit 3; fi
for spec in degraded degraded ready; do
  export HOME="$ROOT/$spec home"
  set_macos_spec_config
  [[ -d $HOME/.muniment ]]
  env -i HOME="$LOGIN_HOME" "$NODE" "$ROOT/probe.mjs"
  printf '%s' "$spec" >"$HOME/.muniment/local-mode"
  env -i HOME="$LOGIN_HOME" "$NODE" "$ROOT/probe.mjs"
  rm "$HOME/.muniment/local-mode"
done
for invalid in '' relative "$LOGIN_HOME"; do
  HOME=$invalid
  if set_macos_spec_config; then exit 4; fi
done
restore_macos_spec_config
restore_macos_spec_config
`)
      expect(result.status, result.stderr).toBe(0)
      expect(result.stdout.trim().split('\n').map(JSON.parse)).toEqual(
        ['degraded', 'degraded', 'ready'].flatMap((spec) => {
          const state = fs.realpathSync(path.join(directory, `${spec} home/.muniment`))
          return [{ state, marker: null }, { state, marker: spec }]
        }),
      )
      if (original === 'absent') expect(fs.existsSync(loginState)).toBe(false)
      else if (original === 'directory') {
        expect(fs.lstatSync(loginState).isDirectory()).toBe(true)
        expect(fs.statSync(loginState).mode & 0o777).toBe(0o700)
        expect(fs.statSync(path.join(loginState, 'local-mode')).mode & 0o777).toBe(0o600)
        expect(fs.readFileSync(path.join(loginState, 'local-mode'), 'utf8')).toBe('login marker')
      } else {
        expect(fs.readlinkSync(loginState)).toBe(external)
        if (original === 'symlink') expect(fs.readFileSync(path.join(external, 'local-mode'), 'utf8')).toBe('login marker')
      }
      expect(fs.readdirSync(path.dirname(loginState))).not.toContainEqual(expect.stringMatching(/^\.muniment-wdio-state\./))
    },
  )

  it.skipIf(process.platform === 'win32')('preserves the original state when a save or link fails', () => {
    const { loginState, shell } = fixture()
    fs.mkdirSync(loginState)
    fs.writeFileSync(path.join(loginState, 'local-mode'), 'original')
    const result = shell(`
for invalid in '' relative; do
  HOME=$invalid
  if save_macos_spec_config; then exit 2; fi
done
HOME=$LOGIN_HOME
mv() { return 1; }
if save_macos_spec_config; then exit 3; fi
restore_macos_spec_config
unset -f mv
save_macos_spec_config
HOME="$ROOT/spec home"
ln() { return 1; }
if set_macos_spec_config; then exit 4; fi
restore_macos_spec_config
`)
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(loginState, 'local-mode'), 'utf8')).toBe('original')
  })
  it.skipIf(process.platform === 'win32')('isolates default Home for each spec and restores the signed-smoke Home', () => {
    const { directory, loginHome, shell } = fixture()
    const home = path.join(loginHome, 'Documents', 'muniment')
    fs.mkdirSync(home, { recursive: true })
    fs.writeFileSync(path.join(home, 'preserved.txt'), 'signed smoke')
    const result = shell(`
save_macos_spec_config
for spec in first second; do
  HOME="$ROOT/$spec"
  set_macos_spec_config
  [[ ! -e $macos_default_home ]]
  mkdir -p "$macos_default_home"
  printf '%s' "$spec" > "$macos_default_home/output.txt"
done
restore_macos_spec_config
`)
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(home, 'preserved.txt'), 'utf8')).toBe('signed smoke')
    expect(fs.readdirSync(home)).toEqual(['preserved.txt'])
    for (const [index, spec] of ['first', 'second'].entries()) {
      expect(fs.readFileSync(path.join(directory, spec, '.muniment', `default-home-${index + 1}`, 'output.txt'), 'utf8')).toBe(spec)
    }
  })

})

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
  const loginConfig = path.join(loginHome, 'Library/Application Support/ai.muniment.desktop')
  fs.mkdirSync(path.dirname(loginConfig), { recursive: true })
  const shell = (script) => spawnSync('bash', ['-c', `
set -eu
source test/e2e/support/macos-spec-config.sh
${script}
`], {
    encoding: 'utf8', timeout: 10_000,
    env: { ...process.env, HOME: loginHome, LOGIN_HOME: loginHome, ROOT: directory, NODE: process.execPath },
  })
  return { directory, loginHome, loginConfig, shell }
}

describe('macOS local mode config', () => {
  it('keeps desktop reads and writes on the runtime resolver', () => {
    const desktop = fs.readFileSync('src-tauri/src/local_mode.rs', 'utf8')
    const runtime = fs.readFileSync('src-tauri/runtime/src/directories.rs', 'utf8')
    const core = fs.readFileSync('src-tauri/core/src/local_mode.rs', 'utf8')
    const resolver = desktop.slice(desktop.indexOf('fn config_directory'), desktop.indexOf('pub(crate) fn is_active'))
    expect(resolver).toMatch(/#\[cfg\(target_os = "macos"\)\][\s\S]*muniment_core::local_mode::macos_config_directory\(\)/)
    expect(desktop).toMatch(/is_local_mode\(&config_directory\(\s*app,?\s*\)\?\)/)
    expect(desktop).toContain('set_local_mode(&config_directory(&app)?, true)')
    expect(desktop).toContain('set_local_mode(&config_directory(&app)?, false)')
    expect(runtime).toMatch(/#\[cfg\(target_os = "macos"\)\]\npub fn config_directory\(\)[\s\S]*?muniment_core::local_mode::macos_config_directory\(/)
    expect(core).toContain('path.join("Library/Application Support")')
    expect(core).toContain('path.join("ai.muniment.desktop")')
  })

  it.skipIf(process.platform === 'win32').each(['absent', 'directory', 'symlink', 'dangling symlink'])(
    'shares each spec marker without the SSH environment and restores an original %s', (original) => {
      const { directory, loginConfig, shell } = fixture()
      const external = path.join(directory, 'external config')
      if (original !== 'absent') {
        if (original !== 'dangling symlink') {
          fs.mkdirSync(external, { mode: 0o700 })
          fs.writeFileSync(path.join(external, 'local-mode'), 'login marker', { mode: 0o600 })
        }
        if (original === 'directory') fs.renameSync(external, loginConfig)
        else fs.symlinkSync(external, loginConfig)
      }
      const probe = path.join(directory, 'probe.mjs')
      fs.writeFileSync(probe, `
import fs from 'node:fs'
import path from 'node:path'
if (process.env.XDG_CONFIG_HOME !== undefined) process.exit(2)
const config = path.join(process.env.HOME, 'Library/Application Support/ai.muniment.desktop')
const marker = path.join(config, 'local-mode')
console.log(JSON.stringify({ config: fs.realpathSync(config), marker: fs.existsSync(marker) ? fs.readFileSync(marker, 'utf8') : null }))
`)
      const result = shell(`
restore_macos_spec_config
if set_macos_spec_config; then exit 2; fi
save_macos_spec_config
if save_macos_spec_config; then exit 3; fi
export XDG_DATA_HOME=/Users/login/.local/share
for spec in degraded degraded ready; do
  export HOME="$ROOT/$spec home"
  set_macos_spec_config
  [[ $XDG_CONFIG_HOME == "$HOME/Library/Application Support" ]]
  [[ $XDG_DATA_HOME == /Users/login/.local/share ]]
  env -i HOME="$LOGIN_HOME" "$NODE" "$ROOT/probe.mjs"
  printf '%s' "$spec" >"$XDG_CONFIG_HOME/ai.muniment.desktop/local-mode"
  env -i HOME="$LOGIN_HOME" "$NODE" "$ROOT/probe.mjs"
  rm "$XDG_CONFIG_HOME/ai.muniment.desktop/local-mode"
done
for invalid in '' relative "$LOGIN_HOME" "$LOGIN_HOME/Library/Application Support/ai.muniment.desktop"; do
  HOME=$invalid
  if set_macos_spec_config; then exit 4; fi
done
restore_macos_spec_config
restore_macos_spec_config
`)
      expect(result.status, result.stderr).toBe(0)
      expect(result.stdout.trim().split('\n').map(JSON.parse)).toEqual(
        ['degraded', 'degraded', 'ready'].flatMap((spec) => {
          const config = fs.realpathSync(path.join(directory, `${spec} home/Library/Application Support/ai.muniment.desktop`))
          return [{ config, marker: null }, { config, marker: spec }]
        }),
      )
      if (original === 'absent') expect(fs.existsSync(loginConfig)).toBe(false)
      else if (original === 'directory') {
        expect(fs.lstatSync(loginConfig).isDirectory()).toBe(true)
        expect(fs.statSync(loginConfig).mode & 0o777).toBe(0o700)
        expect(fs.statSync(path.join(loginConfig, 'local-mode')).mode & 0o777).toBe(0o600)
        expect(fs.readFileSync(path.join(loginConfig, 'local-mode'), 'utf8')).toBe('login marker')
      } else {
        expect(fs.readlinkSync(loginConfig)).toBe(external)
        if (original === 'symlink') expect(fs.readFileSync(path.join(external, 'local-mode'), 'utf8')).toBe('login marker')
      }
      expect(fs.readdirSync(path.dirname(loginConfig))).not.toContainEqual(expect.stringMatching(/^\.muniment-wdio-config\./))
    },
  )

  it.skipIf(process.platform === 'win32')('preserves the original config when a save or link fails', () => {
    const { loginConfig, shell } = fixture()
    fs.mkdirSync(loginConfig)
    fs.writeFileSync(path.join(loginConfig, 'local-mode'), 'original')
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
    expect(fs.readFileSync(path.join(loginConfig, 'local-mode'), 'utf8')).toBe('original')
    expect(fs.readdirSync(path.dirname(loginConfig))).toEqual(['ai.muniment.desktop'])
  })

  it.skipIf(process.platform === 'win32').each(['mv', 'ln'])(
    'restores the original config after a signal interrupts %s', (command) => {
      const { loginConfig, shell } = fixture()
      fs.mkdirSync(loginConfig)
      fs.writeFileSync(path.join(loginConfig, 'local-mode'), 'original')
      const result = shell(`
trap 'trap - TERM; unset -f ${command}; restore_macos_spec_config; exit 0' TERM
${command}() { command ${command} "$@"; kill -TERM $$; }
save_macos_spec_config
HOME="$ROOT/spec home"
set_macos_spec_config
exit 2
`)
      expect(result.status, result.stderr).toBe(0)
      expect(fs.readFileSync(path.join(loginConfig, 'local-mode'), 'utf8')).toBe('original')
      expect(fs.readdirSync(path.dirname(loginConfig))).toEqual(['ai.muniment.desktop'])
    },
  )

  it.skipIf(process.platform === 'win32').each(['directory', 'symlink'])(
    'refuses to remove a %s that replaces the spec link', (replacement) => {
      const { loginConfig, shell } = fixture()
      fs.mkdirSync(loginConfig)
      fs.writeFileSync(path.join(loginConfig, 'local-mode'), 'original')
      const result = shell(`
save_macos_spec_config
HOME="$ROOT/spec home"
set_macos_spec_config
rm "$macos_runtime_config"
${replacement === 'directory' ? 'mkdir "$macos_runtime_config"' : 'ln -s "$ROOT/foreign config" "$macos_runtime_config"'}
if set_macos_spec_config; then exit 2; fi
if restore_macos_spec_config; then exit 3; fi
[[ $(cat "$saved_macos_config/original/local-mode") == original ]]
rm -${replacement === 'directory' ? 'd' : 'f'} "$macos_runtime_config"
restore_macos_spec_config
`)
      expect(result.status, result.stderr).toBe(0)
      expect(fs.readFileSync(path.join(loginConfig, 'local-mode'), 'utf8')).toBe('original')
    },
  )

  it.skipIf(process.platform === 'win32')('links the login home Pi agent directory to the spec home and restores it', () => {
    const { directory, loginHome, shell } = fixture()
    const loginAgent = path.join(loginHome, '.pi/agent')
    fs.mkdirSync(loginAgent, { recursive: true })
    fs.writeFileSync(path.join(loginAgent, 'models.json'), 'login models')
    const result = shell(`
save_macos_spec_config
for spec in degraded ready; do
  export HOME="$ROOT/$spec home"
  set_macos_spec_config
  printf '%s' "$spec" >"$HOME/.pi/agent/models.json"
  [[ -L "$LOGIN_HOME/.pi/agent" ]]
  [[ $(readlink "$LOGIN_HOME/.pi/agent") == "$HOME/.pi/agent" ]]
  [[ $(cat "$LOGIN_HOME/.pi/agent/models.json") == "$spec" ]]
done
HOME=$LOGIN_HOME
if set_macos_spec_config; then exit 2; fi
restore_macos_spec_config
`)
    expect(result.status, result.stderr).toBe(0)
    expect(fs.lstatSync(loginAgent).isSymbolicLink()).toBe(false)
    expect(fs.readFileSync(path.join(loginAgent, 'models.json'), 'utf8')).toBe('login models')
    for (const spec of ['degraded', 'ready']) {
      expect(fs.readFileSync(path.join(directory, `${spec} home/.pi/agent/models.json`), 'utf8')).toBe(spec)
    }
    expect(fs.readdirSync(path.join(loginHome, 'Library/Application Support'))).toEqual([])
  })

  it('keeps the installed probe marker in the runtime config directory', () => {
    const runner = fs.readFileSync('test/e2e/runner/macos.sh', 'utf8')
    expect(runner).toContain('runtime_config_root=${XDG_CONFIG_HOME:-}')
    expect(runner).toContain('[[ $runtime_config_root == /* ]] || runtime_config_root="$HOME/Library/Application Support"')
    expect(runner).toContain('runtime_config=${MUNIMENT_E2E_RUNTIME_CONFIG:-"$runtime_config_root/ai.muniment.desktop"}')
  })

  it('sets the config after cleanup and restores it before state removal', () => {
    const runner = fs.readFileSync('test/e2e/runner/macos-wdio.sh', 'utf8')
    expect(runner.indexOf('cleanup_step stop-before-spec stop_app')).toBeLessThan(runner.indexOf('set_macos_spec_config || return 1'))
    expect(runner.indexOf('set_macos_spec_config || return 1')).toBeLessThan(runner.indexOf('run_step "spec-$spec"'))
    expect(runner.indexOf('run_step stop-before-install stop_app')).toBeLessThan(runner.indexOf('run_step save-config-directory'))
    expect(runner.indexOf('run_step save-config-directory')).toBeLessThan(runner.indexOf('export HOME='))
    expect(runner.indexOf('cleanup_step stop-app stop_app')).toBeLessThan(runner.indexOf('cleanup_step restore-config-directory'))
    expect(runner.indexOf('cleanup_step restore-config-directory')).toBeLessThan(runner.indexOf('cleanup_step remove-raw'))
    expect(fs.readFileSync('test/e2e/support/macos-spec-config.sh', 'utf8')).not.toContain('launchctl')
    const main = fs.readFileSync('src-tauri/runtime/src/main.rs', 'utf8')
    expect(main).toContain('started version={} state_directory={} config_directory={} endpoint={}')
    expect(main).toContain('std::fs::canonicalize(&config_directory)')
  })
})

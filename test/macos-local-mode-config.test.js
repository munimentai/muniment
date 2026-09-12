import { afterEach, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const temporary = []
afterEach(() => {
  for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true })
})

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

  it.skipIf(process.platform === 'win32').each(['', '/Users/login/custom config'])('shares each spec config and restores launchd config %j', (previous) => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-macos-config-'))
    temporary.push(directory)
    const launchctl = path.join(directory, 'launchctl')
    fs.writeFileSync(launchctl, `#!/usr/bin/env bash
set -eu
[[ $2 == XDG_CONFIG_HOME ]]
case $1 in
  getenv) [[ -f $STATE ]] && cat "$STATE" ;;
  setenv) printf '%s' "$3" >"$STATE" ;;
  unsetenv) rm -f "$STATE" ;;
  *) exit 2 ;;
esac
`, { mode: 0o700 })
    const state = path.join(directory, 'environment')
    if (previous) fs.writeFileSync(state, previous)
    const result = spawnSync('bash', ['-c', `
set -eu
source test/e2e/support/macos-spec-config.sh
restore_macos_spec_config
save_macos_spec_config
export XDG_DATA_HOME=/Users/login/.local/share
for spec in degraded ready; do
  export HOME="$ROOT/$spec home"
  set_macos_spec_config
  [[ $XDG_CONFIG_HOME == "$HOME/Library/Application Support" ]]
  [[ $("$MUNIMENT_E2E_LAUNCHCTL" getenv XDG_CONFIG_HOME) == "$XDG_CONFIG_HOME" ]]
  [[ $XDG_DATA_HOME == /Users/login/.local/share ]]
done
HOME=relative
if set_macos_spec_config; then exit 3; fi
HOME="$ROOT/failed home"
saved_launchctl=$MUNIMENT_E2E_LAUNCHCTL
MUNIMENT_E2E_LAUNCHCTL=/bin/false
if set_macos_spec_config; then exit 4; fi
if restore_macos_spec_config; then exit 5; fi
MUNIMENT_E2E_LAUNCHCTL=$saved_launchctl
restore_macos_spec_config
`], {
      encoding: 'utf8',
      env: { ...process.env, MUNIMENT_E2E_LAUNCHCTL: launchctl, ROOT: directory, STATE: state },
    })
    expect(result.stderr).toBe('')
    expect(result.status).toBe(0)
    expect(fs.existsSync(state) ? fs.readFileSync(state, 'utf8') : '').toBe(previous)
  })

  it('keeps the installed probe marker in the runtime config directory', () => {
    const runner = fs.readFileSync('test/e2e/runner/macos.sh', 'utf8')
    expect(runner).toContain('runtime_config_root=${XDG_CONFIG_HOME:-}')
    expect(runner).toContain('[[ $runtime_config_root == /* ]] || runtime_config_root="$HOME/Library/Application Support"')
    expect(runner).toContain('runtime_config=${MUNIMENT_E2E_RUNTIME_CONFIG:-"$runtime_config_root/ai.muniment.desktop"}')
  })

  it('sets launchd config after cleanup and restores it before state removal', () => {
    const runner = fs.readFileSync('test/e2e/runner/macos-wdio.sh', 'utf8')
    expect(runner.indexOf('cleanup_step stop-before-spec stop_app')).toBeLessThan(runner.indexOf('set_macos_spec_config || return 1'))
    expect(runner.indexOf('set_macos_spec_config || return 1')).toBeLessThan(runner.indexOf('run_step "spec-$spec"'))
    expect(runner.indexOf('run_step save-config-environment')).toBeLessThan(runner.indexOf('export HOME='))
    expect(runner.indexOf('cleanup_step stop-app stop_app')).toBeLessThan(runner.indexOf('cleanup_step restore-config-environment'))
    expect(runner.indexOf('cleanup_step restore-config-environment')).toBeLessThan(runner.indexOf('cleanup_step remove-raw'))
  })
})

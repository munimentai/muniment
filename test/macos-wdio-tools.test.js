import { describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const runner = fs.readFileSync('test/e2e/runner/macos-wdio.sh', 'utf8')
const functions = runner.slice(runner.indexOf('run_step()'), runner.indexOf('\nstart_ollama_forward()'))
const setup = runner.slice(runner.indexOf('run_step install-dependencies'), runner.indexOf('\napp_binary='))

describe.skipIf(process.platform === 'win32')('macOS WDIO build tools', () => {
  it.each([0, 42])('inherits prepared tools and stops on setup failure %s', status => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'macos-wdio-tools-'))
    try {
      fs.mkdirSync(path.join(root, 'scripts'))
      fs.mkdirSync(path.join(root, 'tools'))
      for (const name of ['cmake', 'ninja']) {
        fs.writeFileSync(path.join(root, 'tools', name), `#!/bin/sh\necho prepared-${name}\n`, { mode: 0o755 })
      }
      fs.writeFileSync(path.join(root, 'scripts/prepare-cef-macos.sh'), status
        ? `return ${status}\n`
        : 'set -euo pipefail\nexport PATH="$PWD/tools:$PATH"\n')
      const result = spawnSync('bash', ['-c', `
set -uo pipefail
raw="$PWD" status=0 first_failed_step=none
${functions}
require_tools() {
  [[ $(cmake) == prepared-cmake && $(ninja) == prepared-ninja ]] || return 53
  printf 'tools-ready:%s\\n' "$1" >> "$PWD/calls"
}
npm() { [[ $1 == ci ]] || require_tools app; }
rustup() { require_tools targets; }
node() { require_tools runtime; }
${setup}
[[ $- != *e* ]] || exit 54
`], { cwd: root, encoding: 'utf8', timeout: 10_000 })
      expect(result.status, result.stderr).toBe(status)
      const calls = path.join(root, 'calls')
      if (status === 0) {
        expect(fs.readFileSync(calls, 'utf8')).toBe('tools-ready:targets\ntools-ready:runtime\ntools-ready:app\n')
      } else expect(fs.existsSync(calls)).toBe(false)
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  })
})

// The child keeps isolated app state but uses the login keychain preferences.
it.skipIf(process.platform === 'win32')('launches the app with its login home and isolated state', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'macos-wdio-home-'))
  try {
    const app = path.join(root, 'app')
    const log = path.join(root, 'app.log')
    fs.writeFileSync(app, '#!/bin/sh\nprintf "%s\\n" "$HOME" "$MUNIMENT_STATE_DIR" "$MUNIMENT_E2E_HOME_PATH"\n', { mode: 0o755 })
    const result = spawnSync('sh', ['test/e2e/support/macos-wdio-app.sh'], {
      encoding: 'utf8', env: { ...process.env, HOME: '/isolated/spec-home',
        MUNIMENT_E2E_LOGIN_HOME: '/login/home', MUNIMENT_STATE_DIR: '/login/home/.muniment',
        MUNIMENT_E2E_HOME_PATH: '/isolated/workspace', MUNIMENT_E2E_REAL_APP_BINARY: app,
        MUNIMENT_E2E_DRIVER_APP_LOG: log },
    })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(log, 'utf8')).toBe('/login/home\n/login/home/.muniment\n/isolated/workspace\n')
    expect(runner.indexOf('export MUNIMENT_E2E_LOGIN_HOME="$HOME"')).toBeLessThan(runner.indexOf('export HOME="$state_root/degraded"'))
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

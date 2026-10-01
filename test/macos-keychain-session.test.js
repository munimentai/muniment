import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'

// Exercise both audit sessions without touching the host Keychains.
describe.skipIf(process.platform === 'win32')('macOS CI keychain session', () => {
  it.each([
    ['success', 0], ['runner failure', 7], ['setup failure', 1],
    ['console unlock failure', 1], ['selection failure', 1],
    ['missing console', 1], ['without passwordless sudo', 0], ['create failure', 1],
    ['bootstrap failure', 1], ['console settings failure', 1], ['console timeout', 1],
    ['partial create failure', 1], ['bootout failure', 1], ['runner signal', 143], ['cleanup default', 1], ['cleanup list', 1], ['cleanup delete', 1], ['runner and bootout failure', 1],
  ])('The runner restores the Keychains after %s.', (mode, expected) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'keychain fixture & <test> "quote"\'-'))
    const bin = path.join(root, 'bin')
    fs.mkdirSync(bin)
    const log = path.join(root, 'operations')
    fs.writeFileSync(log, '')
    fs.writeFileSync(path.join(bin, 'sudo'), `#!/usr/bin/env bash
printf 'sudo must not run.\\n' > "$TEST_ROOT/sudo-called"
exit 1
`, { mode: 0o700 })
    fs.writeFileSync(path.join(bin, 'launchctl'), `#!${process.execPath}
const fs = require('node:fs')
const path = require('node:path')
const assert = require('node:assert/strict')
const { spawn } = require('node:child_process')
const [op, domain, plist] = process.argv.slice(2)
const root = process.env.TEST_ROOT
const mode = process.env.TEST_MODE
const gui = 'gui/' + process.getuid()
if (op === 'print') {
  assert.equal(domain, gui)
  process.exit(mode === 'missing console' ? 1 : 0)
}
if (op === 'bootstrap') {
  assert.equal(domain, gui)
  const source = fs.readFileSync(plist, 'utf8')
  const decode = (text) => text.replace(/&(amp|lt|gt|quot|apos);/g, (_, entity) => ({
    amp: '&', lt: '<', gt: '>', quot: '"', apos: "'",
  })[entity])
  const label = source.match(/<key>Label<\\/key><string>([^<]+)<\\/string>/)[1]
  assert.match(label, /^ai\\.muniment\\.e2e-keychain\\.[a-zA-Z0-9]+$/)
  assert.match(source, /<key>LimitLoadToSessionType<\\/key><string>Aqua<\\/string>/)
  assert.match(source, /<key>RunAtLoad<\\/key><true\\/>/)
  const args = [...source.match(/<array>([\\s\\S]+)<\\/array>/)[1].matchAll(/<string>([^<]*)<\\/string>/g)]
    .map((match) => decode(match[1]))
  const work = path.dirname(plist)
  assert.deepEqual(args, ['/bin/bash', path.join(work, 'setup.sh'), work, path.join(root, 'bin/security')])
  assert.equal(fs.statSync(path.join(work, 'password')).mode & 0o777, 0o600)
  assert.ok(!source.includes(fs.readFileSync(path.join(work, 'password'), 'utf8').trim()))
  fs.writeFileSync(path.join(root, 'agent-label'), label)
  if (mode === 'bootstrap failure') process.exit(1)
  if (mode !== 'console timeout') {
    const child = spawn(args[0], args.slice(1), {
      detached: true, stdio: 'ignore', env: { ...process.env, TEST_CONTEXT: 'gui' },
    })
    fs.writeFileSync(path.join(root, 'agent-pid'), String(child.pid))
    child.unref()
  }
} else if (op === 'bootout') {
  assert.equal(domain, gui + '/' + fs.readFileSync(path.join(root, 'agent-label'), 'utf8'))
  fs.writeFileSync(path.join(root, 'agent-stopped'), '')
  const pid = path.join(root, 'agent-pid')
  if (fs.existsSync(pid)) {
    try { process.kill(-Number(fs.readFileSync(pid, 'utf8')), 'SIGTERM') }
    catch (error) { if (error.code !== 'ESRCH') throw error }
  }
  if (['bootout failure', 'runner and bootout failure'].includes(mode)) process.exit(1)
} else {
  throw new Error('Unexpected launchctl command: ' + op)
}
`, { mode: 0o700 })
    fs.writeFileSync(path.join(bin, 'sleep'), `#!/usr/bin/env bash
if [[ "$TEST_MODE" != 'console timeout' ]]; then exec /bin/sleep "$@"; fi
`, { mode: 0o700 })
    fs.writeFileSync(path.join(bin, 'security'), `#!/usr/bin/env bash
set -eu
op=$1
context=\${TEST_CONTEXT:-ssh}
printf '%s:%s\\n' "$context" "$op" >> "$TEST_LOG"
case "$op" in
  default-keychain)
    if [[ $# == 3 ]]; then printf '    "/original/login.keychain-db"\\n'
    else
      [[ -f "$TEST_ROOT/agent-stopped" || "\${!#}" != /original/* ]] || exit 91
      printf '%s\\n' "\${!#}" > "$TEST_ROOT/default"; [[ "$TEST_MODE" != 'cleanup default' || "\${!#}" != /original/* ]] || exit 1
    fi ;;
  list-keychains)
    if [[ $# == 3 ]]; then printf '    "/original/login.keychain-db"\\n    "/original/other.keychain-db"\\n'
    else
      shift 4
      if [[ "$1" != /original/* ]]; then
        [[ -f "$TEST_ROOT/unlocked-gui" && -f "$TEST_ROOT/unlocked-ssh" ]] || exit 91
        [[ "$TEST_MODE" != 'selection failure' ]] || exit 1
      fi
      printf '%s\\n' "$@" > "$TEST_ROOT/list"; [[ "$TEST_MODE" != 'cleanup list' || "$1" != /original/* ]] || exit 1
    fi ;;
  create-keychain)
    # Creation exposes the Keychain to Spotlight and unlocks only the creator's audit session.
    [[ "$context" == gui ]] || { echo 'Spotlight would request the Keychain password.' >&2; exit 91; }
    [[ "$TEST_MODE" != 'create failure' ]] || exit 1
    touch "\${!#}" "$TEST_ROOT/unlocked-gui"
    printf '%s' "\${!#}" > "$TEST_ROOT/created"
    [[ "$TEST_MODE" != 'partial create failure' ]] || exit 1 ;;
  set-keychain-settings)
    [[ $# == 2 && -f "$TEST_ROOT/unlocked-$context" ]] || exit 91
    [[ "$TEST_MODE" != 'console settings failure' ]] ;;
  unlock-keychain)
    if [[ "$context" == gui ]]; then [[ "$TEST_MODE" != 'console unlock failure' ]] || exit 1
    else [[ "$TEST_MODE" != 'setup failure' ]] || exit 1; fi
    touch "$TEST_ROOT/unlocked-$context" ;;
  delete-keychain) [[ "$TEST_MODE" != 'cleanup delete' ]] || exit 1; rm -f -- "$2" ;;
  *) exit 90 ;;
esac
`, { mode: 0o700 })
    try {
      const result = spawnSync('bash', ['test/e2e/support/macos-keychain-session.sh', 'sh', '-c', `
        touch "$TEST_ROOT/runner-started"
        test "$(cat "$TEST_ROOT/default")" = "$(cat "$TEST_ROOT/created")" || exit 92
        test -f "$TEST_ROOT/unlocked-gui" && test -f "$TEST_ROOT/unlocked-ssh" || exit 92
        test ! -f "$(dirname "$(cat "$TEST_ROOT/created")")/password" || exit 92
        if [ "$TEST_MODE" = 'runner failure' ] || [ "$TEST_MODE" = 'runner and bootout failure' ]; then exit 7; fi
        if [ "$TEST_MODE" = 'runner signal' ]; then kill -TERM "$PPID"; fi
      `], {
        cwd: process.cwd(), encoding: 'utf8', timeout: 15_000,
        env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, TMPDIR: root, TEST_LOG: log, TEST_ROOT: root, TEST_MODE: mode },
      })
      expect(result.error).toBeUndefined()
      expect(result.status, result.stderr).toBe(expected)
      expect(fs.existsSync(path.join(root, 'sudo-called'))).toBe(false)
      const operations = fs.readFileSync(log, 'utf8').trim().split('\n')
      const created = fs.existsSync(path.join(root, 'created'))
      const requested = mode !== 'missing console'
      expect(fs.existsSync(path.join(root, 'agent-stopped'))).toBe(requested)
      expect(created).toBe(requested && !['bootstrap failure', 'create failure', 'console timeout'].includes(mode))
      if (created) {
        expect(fs.readFileSync(path.join(root, 'default'), 'utf8').trim()).toBe('/original/login.keychain-db')
        expect(fs.readFileSync(path.join(root, 'list'), 'utf8').trim().split('\n')).toEqual(['/original/login.keychain-db', '/original/other.keychain-db'])
        expect(fs.existsSync(fs.readFileSync(path.join(root, 'created'), 'utf8'))).toBe(false)
        expect(operations.slice(-3)).toEqual(['ssh:default-keychain', 'ssh:list-keychains', 'ssh:delete-keychain'])
      }
      const ran = ['success', 'without passwordless sudo', 'runner failure', 'bootout failure', 'runner signal', 'cleanup default', 'cleanup list', 'cleanup delete', 'runner and bootout failure'].includes(mode)
      expect(fs.existsSync(path.join(root, 'runner-started'))).toBe(ran)
      if (ran) {
        expect(operations.slice(0, -3)).toEqual([
          'ssh:default-keychain', 'ssh:list-keychains', 'gui:create-keychain',
          'gui:set-keychain-settings', 'gui:unlock-keychain', 'ssh:unlock-keychain',
          'ssh:list-keychains', 'ssh:default-keychain',
        ])
      }
      const reason = mode === 'runner failure' ? 'probe-failed' : ['bootout failure', 'runner and bootout failure'].includes(mode) ? 'cleanup-agent'
        : mode.startsWith('cleanup ') ? mode.replace(' ', '-') : mode === 'console timeout' ? 'setup-timeout' : 'setup-failed'
      if (expected !== 0 && mode !== 'runner signal') expect(result.stderr).toContain(`macos-keychain-session: reason=${reason}`)
      if (mode === 'runner failure') expect(result.stderr).not.toContain('reason=cleanup-')
      if (mode === 'runner and bootout failure') expect(result.stderr).toContain('reason=probe-failed')
      if (expected === 0) expect(result.stderr).not.toContain('macos-keychain-session: reason=')
      expect(fs.readdirSync(root).filter((name) => name.startsWith('muniment-keychain.'))).toEqual([])
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  })
})

import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'

// Exercise keychain selection and cleanup without touching the host keychains.
describe.skipIf(process.platform === 'win32')('macOS CI keychain session', () => {
  it.each([['success', 0], ['runner failure', 7], ['setup failure', 1]])('restores the keychains after %s', (mode, expected) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'keychain-fixture-'))
    const bin = path.join(root, 'bin')
    fs.mkdirSync(bin)
    const log = path.join(root, 'operations')
    fs.writeFileSync(path.join(bin, 'security'), `#!/usr/bin/env bash
set -eu
op=$1
printf '%s\\n' "$op" >> "$TEST_LOG"
case "$op" in
  default-keychain) if [[ $# == 3 ]]; then printf '    "/original/login.keychain-db"\\n'; else printf '%s\\n' "\${!#}" > "$TEST_ROOT/default"; fi ;;
  list-keychains) if [[ $# == 3 ]]; then printf '    "/original/login.keychain-db"\\n    "/original/other.keychain-db"\\n'; else shift 4; printf '%s\\n' "$@" > "$TEST_ROOT/list"; fi ;;
  create-keychain) touch "\${!#}"; printf '%s' "\${!#}" > "$TEST_ROOT/created" ;;
  unlock-keychain) [[ "$TEST_MODE" != 'setup failure' ]] ;;
  delete-keychain) rm -f -- "$2" ;;
esac
`, { mode: 0o700 })
    try {
      const result = spawnSync('bash', ['test/e2e/support/macos-keychain-session.sh', 'sh', '-c', mode === 'runner failure' ? 'exit 7' : 'test "$(cat "$TEST_ROOT/default")" = "$(cat "$TEST_ROOT/created")"'], {
        cwd: process.cwd(), encoding: 'utf8', env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, TMPDIR: root, TEST_LOG: log, TEST_ROOT: root, TEST_MODE: mode },
      })
      expect(result.status, result.stderr).toBe(expected)
      expect(fs.readFileSync(path.join(root, 'default'), 'utf8').trim()).toBe('/original/login.keychain-db')
      expect(fs.readFileSync(path.join(root, 'list'), 'utf8').trim().split('\n')).toEqual(['/original/login.keychain-db', '/original/other.keychain-db'])
      expect(fs.existsSync(fs.readFileSync(path.join(root, 'created'), 'utf8'))).toBe(false)
      expect(fs.readFileSync(log, 'utf8').trim().split('\n').slice(-3)).toEqual(['default-keychain', 'list-keychains', 'delete-keychain'])
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  })
})

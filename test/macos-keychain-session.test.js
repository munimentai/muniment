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
    ['missing console', 1], ['sudo failure', 1], ['create failure', 1],
  ])('The runner restores the Keychains after %s.', (mode, expected) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'keychain-fixture-'))
    const bin = path.join(root, 'bin')
    fs.mkdirSync(bin)
    const log = path.join(root, 'operations')
    fs.writeFileSync(log, '')
    fs.writeFileSync(path.join(bin, 'sudo'), `#!/usr/bin/env bash
set -eu
[[ "$1" == -n ]] || exit 90
shift
[[ "$TEST_MODE" != 'sudo failure' ]] || exit 1
if [[ "$1" == -H ]]; then
  [[ "$2" == -u && "$3" == "#$(id -u)" && "$4" == security ]] || exit 90
  shift 3
fi
exec "$@"
`, { mode: 0o700 })
    fs.writeFileSync(path.join(bin, 'launchctl'), `#!/usr/bin/env bash
set -eu
case "$1" in
  print) [[ "$2" == "gui/$(id -u)" && "$TEST_MODE" != 'missing console' ]] ;;
  asuser)
    [[ "$2" == "$(id -u)" ]] || exit 90
    shift 2
    TEST_CONTEXT=gui exec "$@" ;;
  *) exit 90 ;;
esac
`, { mode: 0o700 })
    fs.writeFileSync(path.join(bin, 'security'), `#!/usr/bin/env bash
set -eu
op=$1
context=\${TEST_CONTEXT:-ssh}
printf '%s:%s\\n' "$context" "$op" >> "$TEST_LOG"
case "$op" in
  default-keychain)
    if [[ $# == 3 ]]; then printf '    "/original/login.keychain-db"\\n'
    else printf '%s\\n' "\${!#}" > "$TEST_ROOT/default"; fi ;;
  list-keychains)
    if [[ $# == 3 ]]; then printf '    "/original/login.keychain-db"\\n    "/original/other.keychain-db"\\n'
    else
      shift 4
      if [[ "$1" != /original/* ]]; then
        [[ -f "$TEST_ROOT/unlocked-gui" && -f "$TEST_ROOT/unlocked-ssh" ]] || exit 91
        [[ "$TEST_MODE" != 'selection failure' ]] || exit 1
      fi
      printf '%s\\n' "$@" > "$TEST_ROOT/list"
    fi ;;
  create-keychain)
    # Creation exposes the Keychain to Spotlight and unlocks only the creator's audit session.
    [[ "$context" == gui ]] || { echo 'Spotlight would request the Keychain password.' >&2; exit 91; }
    [[ "$TEST_MODE" != 'create failure' ]] || exit 1
    touch "\${!#}" "$TEST_ROOT/unlocked-gui"
    printf '%s' "\${!#}" > "$TEST_ROOT/created" ;;
  set-keychain-settings) [[ $# == 2 && -f "$TEST_ROOT/unlocked-$context" ]] ;;
  unlock-keychain)
    if [[ "$context" == gui ]]; then [[ "$TEST_MODE" != 'console unlock failure' ]] || exit 1
    else [[ "$TEST_MODE" != 'setup failure' ]] || exit 1; fi
    touch "$TEST_ROOT/unlocked-$context" ;;
  delete-keychain) rm -f -- "$2" ;;
  *) exit 90 ;;
esac
`, { mode: 0o700 })
    try {
      const result = spawnSync('bash', ['test/e2e/support/macos-keychain-session.sh', 'sh', '-c', `
        touch "$TEST_ROOT/runner-started"
        test "$(cat "$TEST_ROOT/default")" = "$(cat "$TEST_ROOT/created")" || exit 92
        test -f "$TEST_ROOT/unlocked-gui" && test -f "$TEST_ROOT/unlocked-ssh" || exit 92
        if [ "$TEST_MODE" = 'runner failure' ]; then exit 7; fi
      `], {
        cwd: process.cwd(), encoding: 'utf8', env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, TMPDIR: root, TEST_LOG: log, TEST_ROOT: root, TEST_MODE: mode },
      })
      expect(result.status, result.stderr).toBe(expected)
      const operations = fs.readFileSync(log, 'utf8').trim().split('\n')
      const created = fs.existsSync(path.join(root, 'created'))
      expect(created).toBe(!['missing console', 'sudo failure', 'create failure'].includes(mode))
      if (created) {
        expect(fs.readFileSync(path.join(root, 'default'), 'utf8').trim()).toBe('/original/login.keychain-db')
        expect(fs.readFileSync(path.join(root, 'list'), 'utf8').trim().split('\n')).toEqual(['/original/login.keychain-db', '/original/other.keychain-db'])
        expect(fs.existsSync(fs.readFileSync(path.join(root, 'created'), 'utf8'))).toBe(false)
        expect(operations.slice(-3)).toEqual(['ssh:default-keychain', 'ssh:list-keychains', 'ssh:delete-keychain'])
      }
      const ran = ['success', 'runner failure'].includes(mode)
      expect(fs.existsSync(path.join(root, 'runner-started'))).toBe(ran)
      if (ran) {
        expect(operations.slice(0, -3)).toEqual([
          'ssh:default-keychain', 'ssh:list-keychains', 'gui:create-keychain',
          'gui:set-keychain-settings', 'gui:unlock-keychain', 'ssh:unlock-keychain',
          'ssh:list-keychains', 'ssh:default-keychain',
        ])
      }
      expect(fs.readdirSync(root).filter((name) => name.startsWith('muniment-keychain.'))).toEqual([])
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  })
})

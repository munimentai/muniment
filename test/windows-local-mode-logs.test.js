import { afterEach, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const temporary = []
const temp = () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-windows-pi-'))
  temporary.push(directory)
  return directory
}
afterEach(() => {
  for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true })
})
// Logs sit under the local application data identifier; state sits in the profile root itself.
const plant = (root, name, text, prefix = 'ai.muniment.desktop') => {
  const file = path.join(root, prefix, name)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, text)
  return file
}
const plantState = (root, name, text) => plant(root, name, text, '.')
const collect = (input) => spawnSync(process.execPath, ['test/e2e/support/windows-local-mode-logs.mjs'], {
  input: JSON.stringify(input), encoding: 'utf8',
})

it('Collects known-folder and redirected Pi logs before another spec replaces the runtime log.', () => {
  const local = temp(); const roaming = temp(); const redirected = temp(); const destination = temp()
  const runtime = plant(local, 'logs/runtime.log', 'muniment-runtime: run_id=local-run pi_spawn started\n')
  plantState(roaming, 'sessions/local.jsonl', '{"type":"session","id":"local-run"}\n')
  plantState(redirected, 'sessions/redirected.jsonl', '{"type":"session","id":"redirected-run"}\n')
  plantState(roaming, 'agent/auth.json', 'private credentials')
  plantState(roaming, 'sessions/ignored.txt', 'private non-session file')
  const result = collect({ localRoots: [local, local, null], profileRoots: [roaming, redirected, roaming, ''], destination })
  expect(result.status, result.stderr).toBe(0)
  const session = fs.readFileSync(path.join(destination, 'pi-local-mode-chat.log'), 'utf8')
  expect(session.match(/Pi wrote session log/g)).toHaveLength(2)
  expect(session).toContain('local-run')
  expect(session).toContain('redirected-run')
  expect(session).not.toContain('private')
  const stderr = fs.readFileSync(path.join(destination, 'pi-local-mode-stderr.log'), 'utf8')
  expect(stderr.match(/run_id=local-run/g)).toHaveLength(1)
  fs.writeFileSync(runtime, 'muniment-runtime: native-auth failure stage=registration error=Persistence\n')
  const relay = spawnSync(process.execPath, ['test/e2e/support/windows-runtime-log-tail.mjs', destination], {
    input: JSON.stringify([local]), encoding: 'utf8',
  })
  expect(relay.status, relay.stderr).toBe(0)
  expect(relay.stdout).toContain('run_id=local-run pi_spawn started')
  expect(relay.stdout).toContain('native-auth failure stage=registration error=Persistence')
})

it('Names missing logs and ignores directories with session file names.', () => {
  const root = temp(); const destination = temp()
  fs.mkdirSync(path.join(root, 'sessions', 'directory.jsonl'), { recursive: true })
  const result = collect({ localRoots: [root], profileRoots: [root], destination })
  expect(result.status, result.stderr).toBe(0)
  expect(fs.readFileSync(path.join(destination, 'pi-local-mode-chat.log'), 'utf8'))
    .toBe('No Pi session log exists for the local mode run.\n')
  expect(fs.readFileSync(path.join(destination, 'pi-local-mode-stderr.log'), 'utf8'))
    .toBe('No runtime log exists for the local mode run.\n')
})

it('Reports collection errors instead of treating an unreadable session directory as missing.', () => {
  const root = temp(); const destination = temp()
  plantState(root, 'sessions', 'not a directory')
  const result = collect({ localRoots: [root], profileRoots: [root], destination })
  expect(result.status).not.toBe(0)
  expect(result.stderr).toContain('ENOTDIR')
})

it('Keeps the run cause beyond 60 lines and redacts the full diagnostic before the relay prints it.', () => {
  const root = temp()
  plant(root, 'logs/runtime.log', [
    'muniment-runtime: run_id=local-run first_event absent cause=timeout',
    'muniment-runtime: run_id=local-run pi_stderr_tail=["Bearer private-token"]',
    ...Array(80).fill('muniment-runtime: desktop session closed'),
  ].join('\n'))
  const result = spawnSync(process.execPath, ['test/e2e/support/windows-runtime-log-tail.mjs'], {
    input: JSON.stringify([root]), encoding: 'utf8',
  })
  expect(result.status, result.stderr).toBe(0)
  expect(result.stdout).toContain('run_id=local-run first_event absent cause=timeout')
  expect(result.stdout).toContain('[REDACTED:bearer-token]')
  expect(result.stdout).not.toContain('private-token')
})

it('Collects local mode logs after failure and before sign-in without changing the task or session start.', () => {
  const runner = fs.readFileSync('test/e2e/runner/windows.ps1', 'utf8')
  const localMode = runner.indexOf("'test/e2e/specs/local-mode-chat.spec.js'")
  const collection = runner.indexOf('    Save-LocalModePiLogs', localMode)
  const signIn = runner.indexOf("'test/e2e/specs/real-sign-in.spec.js'", localMode)
  expect(collection).toBeGreaterThan(localMode)
  expect(collection).toBeLessThan(signIn)
  expect(runner.slice(localMode, collection)).toContain('Save-RunnerFailure $_')
  expect(runner.slice(collection, signIn)).toContain('Save-RunnerFailure $_')
  expect(runner).toContain('profileRoots = @((Get-E2eProfileDirectory))')
  expect(runner).toContain('[Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)')
})

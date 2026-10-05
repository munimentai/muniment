import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { awaitUpdateResult } from './e2e/runner/subscriptions.mjs'
import { collectWindowsUpdate, copyWindowsUpdate, safeMsiLog, updateRuntimeRecovery, windowsStartup, windowsUpdateDiagnostics } from './e2e/support/subscription-windows-update.mjs'

function temporary(run) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-update-diagnostics-'))
  try { return run(root) } finally { fs.rmSync(root, { recursive: true, force: true }) }
}

const state = { parentPid: 123, startup: [{ pid: 456, code: 'profile-restored' }], appAlive: false, runtimeAlive: false, recovered: false }

test('the Windows update restores the runtime only after the MSI app restores its profile', () => {
  assert.equal(updateRuntimeRecovery(state), true)
  for (const override of [{ parentPid: 0 }, { parentPid: 2147483648 }, { parentPid: '123' }, { appAlive: true }, { runtimeAlive: true },
    { startup: [] }, { startup: null }, { startup: [null] }, { appAlive: null }, { runtimeAlive: null }, { recovered: undefined },
    { startup: [{ pid: 123, code: 'profile-restored' }] }, { startup: [{ pid: 456, code: 'started' }] },
    { startup: [{ pid: 456, code: 'job-assign-failed' }] }, { startup: [{ pid: -1, code: 'profile-restored' }] }]) {
    assert.equal(updateRuntimeRecovery({ ...state, ...override }), false)
  }
  assert.throws(() => updateRuntimeRecovery({ ...state, recovered: true }), /restored probe runtime exited/)
})

test('the update wait recovers a runtime closed by MSI without launching another app', async () => {
  let clock = 0, runtimeStarts = 0
  const current = { ...state, appAlive: true, runtimeAlive: true, startup: [] }
  const actions = []
  const result = { pid: 456, phase: 'update-restart', passed: true }
  assert.equal(await awaitUpdateResult(() => runtimeStarts ? result : false, {
    now: () => clock, timeout: 2000,
    wait: async ms => {
      clock += ms
      if (clock === 250) { current.appAlive = false; current.runtimeAlive = false }
      if (clock === 500) current.startup = [{ pid: 456, code: 'started' }]
      if (clock === 750) current.startup.push({ pid: 456, code: 'profile-restored' })
    },
    recover: async () => {
      if (!updateRuntimeRecovery(current)) return
      actions.push('stop-runtime-job', 'start-runtime')
      current.recovered = true
      current.runtimeAlive = true
      runtimeStarts++
    },
  }), result)
  assert.equal(clock, 750)
  assert.equal(runtimeStarts, 1)
  assert.deepEqual(actions, ['stop-runtime-job', 'start-runtime'])
  await assert.rejects(awaitUpdateResult(() => false, { recover: async () => { throw new Error('The runtime could not start.') } }), /could not start/)
})

test('Windows startup diagnostics reject unknown codes, invalid PIDs, partial rows, and linked files', () => temporary(root => {
  const file = path.join(root, 'subscription-probe-startup.log')
  fs.writeFileSync(file, ['pid=123 code=started', 'pid=456 code=started', 'pid=456 code=job-open-failed',
    'pid=0 code=started', 'pid=-1 code=started', 'pid=2147483648 code=started', 'pid=1 code=secret',
    'pid=456 code=profile-restored secret', 'pid=456 code=profile-restored'].join('\n'))
  assert.deepEqual(windowsStartup(root), [{ pid: 123, code: 'started' }, { pid: 456, code: 'started' }, { pid: 456, code: 'job-open-failed' }])
  const receipt = path.join(root, 'subscription-probe-process-456.json')
  for (const value of [null, {}, { pid: 456, exit_code: -1, cleanup: false }, { pid: 456, exit_code: 0x100000000, cleanup: false },
    { pid: 456, exit_code: 'secret', cleanup: false }, { pid: 999, exit_code: 1, cleanup: false }, { pid: 456, exit_code: 1, cleanup: 'false' }]) {
    fs.writeFileSync(receipt, JSON.stringify(value) + '\n')
    assert.equal(windowsUpdateDiagnostics(root, 123).processes[1].observed, false)
  }
  fs.writeFileSync(receipt, JSON.stringify({ pid: 456, exit_code: 0xc0000005, cleanup: false, secret: 'provider text' }) + '\n')
  const result = windowsUpdateDiagnostics(root, 123)
  assert.equal(result.relaunch_started, true)
  assert.deepEqual(result.processes[1], { pid: 456, exit_code: 0xc0000005, cleanup: false, observed: true })
  assert.equal(JSON.stringify(result).includes('provider text'), false)
  fs.rmSync(file)
  assert.equal(windowsUpdateDiagnostics(root, 123).relaunch_started, false)
  if (process.platform !== 'win32') {
    fs.symlinkSync('/etc/passwd', file)
    assert.deepEqual(windowsStartup(root), [])
  }
}))

test('MSI diagnostics keep action results without tokens, provider text, environment values, or user paths', () => temporary(root => {
  const raw = 'Property(S): USERPROFILE = C:\\Users\\private-owner\n' +
    'Property(S): TOKEN = secret-token\nProvider reply: private-provider-text\n' +
    'Action start 12:34:56: InstallFiles.\nAction ended 12:34:57: InstallFiles. Return value 1.\n' +
    'Action start 12:34:58: LaunchApplication.\nAction ended 12:34:59: LaunchApplication. Return value 3.\n' +
    'Action start 12:34:59: secret_token.\n' +
    'MSI (c) (A0:BC) [12:34:59:000]: MainEngineThread is returning 1603\n'
  const expected = 'action=InstallFiles state=start\naction=InstallFiles state=ended result=1\n' +
    'action=LaunchApplication state=start\naction=LaunchApplication state=ended result=3\n' +
    'action=other state=start\nmsiexec_result=1603\n'
  assert.equal(safeMsiLog(raw), expected)
  assert.equal(safeMsiLog('MSI (c) (A0:BC) [12:34:59:000]: MainEngineThread is returning 4294967296\n'), 'No MSI action or result record exists.\n')
  fs.writeFileSync(path.join(root, 'subscription-probe-msi.log'), Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(raw, 'utf16le')]))
  const output = path.join(root, 'artifacts')
  fs.mkdirSync(output)
  collectWindowsUpdate(root, output, 123)
  assert.equal(fs.readFileSync(path.join(output, 'windows-subscription-msi.log'), 'utf8'), expected)
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(output, 'windows-subscription-relaunch.json'))), {
    parent_pid: 123, relaunch_started: false, processes: [{ pid: 123, observed: false, exit_code: null, cleanup: false }], startup: [], observer_error: null,
  })
  // Keep early MSI actions even when a large property dump follows them.
  fs.appendFileSync(path.join(root, 'subscription-probe-msi.log'), Buffer.from('Property(S): PRIVATE = private-provider-text\n'.repeat(2000), 'utf16le'))
  collectWindowsUpdate(root, output, 123)
  assert.equal(fs.readFileSync(path.join(output, 'windows-subscription-msi.log'), 'utf8'), expected)
}))

test('update diagnostics survive host and collection copies without unknown fields or forged values', () => temporary(root => {
  const guest = path.join(root, 'guest'), host = path.join(root, 'host'), collected = path.join(root, 'collected')
  for (const directory of [guest, host, collected]) fs.mkdirSync(directory)
  fs.writeFileSync(path.join(guest, 'windows-subscription-relaunch.json'), JSON.stringify({
    parent_pid: 123, relaunch_started: true, secret: 'provider text',
    startup: [{ pid: 456, code: 'started', private: 'C:\\Users\\owner' }, { pid: 0, code: 'started' }, { pid: 456, code: 'secret' }],
    processes: [{ pid: 456, observed: true, exit_code: 1, cleanup: false, secret: 'token' }, { pid: -1, observed: true, exit_code: 'secret', cleanup: false }],
    observer_error: 'private provider error',
  }) + '\n')
  fs.writeFileSync(path.join(guest, 'windows-subscription-msi.log'), 'action=LaunchApplication state=ended result=1\nmsiexec_result=0\nTOKEN=secret\nC:\\Users\\owner\n')
  copyWindowsUpdate(guest, host)
  fs.rmSync(guest, { recursive: true })
  copyWindowsUpdate(host, collected)
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(collected, 'windows-subscription-relaunch.json'))), {
    parent_pid: 123, relaunch_started: true, startup: [{ pid: 456, code: 'started' }],
    processes: [{ pid: 456, observed: true, exit_code: 1, cleanup: false }], observer_error: null,
  })
  assert.equal(fs.readFileSync(path.join(collected, 'windows-subscription-msi.log'), 'utf8'), 'action=LaunchApplication state=ended result=1\nmsiexec_result=0\n')
}))

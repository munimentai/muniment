import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { subscriptionRedactor, diagnosticTail, nativeFailure, processStatus, profileLogs, transcriptText, linuxRuntimeStatus, readDiagnosticLog, probeProgress } from './e2e/support/subscription-diagnostics.mjs'
import { awaitProbeResult, awaitUpdateResult, equalPayload, isolatedEnvironment, verifyInstalled, writeBlocked } from './e2e/runner/subscriptions.mjs'
import { host, runDesktopCi } from './e2e/runner/subscription-host.mjs'
import { collect } from './e2e/runner/collect-subscriptions.mjs'

const sourceSha = 'a'.repeat(40)
const lease = { provider: 'openai-codex', access: 'private-access-value', account_id: 'private-account-value', expires_ms: Date.now() + 60 * 60_000 }
const leases = JSON.stringify([lease])
const models = JSON.stringify(Array.from({ length: 4 }, (_, index) => ({ family: 'openai', id: `model-${index}` })))
const token = 'private-github-value'
const key = 'private-ssh-value'
const redact = subscriptionRedactor({ leases, values: [token, key] })
const privateValues = [lease.access, lease.account_id, leases, Buffer.from(leases).toString('base64'), token, key]
const assertSafe = text => {
  for (const value of privateValues) assert.equal(text.includes(value), false, 'The log must omit every secret.')
}
function temporary(work) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-diagnostics-'))
  try { return work(root) } finally { fs.rmSync(root, { recursive: true, force: true }) }
}

test('subscription diagnostics redact lease values, encoded payloads, headers, and quoted account fields', () => {
  const message = [...privateValues, JSON.stringify({ access_token: 'unknown-access', account_id: 'unknown-account' }),
    'chatgpt-account-id: unknown-header', 'Authorization: Bearer unknown-bearer',
    'refresh_token=unknown-refresh', '{"authorization":"Basic unknown-basic"}',
    'The runtime exited with status 42.'].join('\n')
  const safe = redact(message)
  assertSafe(safe)
  for (const value of ['unknown-access', 'unknown-account', 'unknown-header', 'unknown-bearer', 'unknown-refresh', 'unknown-basic']) {
    assert.equal(safe.includes(value), false)
  }
  assert.match(safe, /status 42/)
  const invalid = subscriptionRedactor({ leases: leases.slice(0, -1) })(`JSON parser excerpt: ${lease.access.slice(0, -3)}`)
  assert.equal(invalid.includes(lease.access.slice(0, -3)), false)
  const unicode = subscriptionRedactor({ leases: JSON.stringify([{ ...lease, access: '\ud800private-value' }]) })
  assert.equal(unicode('native error \\ud800private-value').includes('private-value'), false)
  assertSafe(diagnosticTail(message, redact, 41))
  const crossing = 'x'.repeat(100) + lease.access + ' end'
  assert.equal(diagnosticTail(crossing, redact, 10).includes('access-value'), false)
})

test('native failures keep command status and discard a truncated secret fragment', () => {
  const error = nativeFailure('msiexec.exe', { status: 1603, stdout: 'admin image failed', stderr: lease.access })
  assert.match(redact(error.message), /msiexec.exe: status=1603/)
  assert.match(redact(error.message), /admin image failed/)
  assertSafe(redact(error.message))
  const timeout = nativeFailure('ssh', { status: null, signal: 'SIGTERM', error: new Error('ETIMEDOUT'), stdout: 'known line\nprivate-acc' })
  assert.match(timeout.message, /ETIMEDOUT/)
  assert.equal(timeout.message.includes('private-acc'), false)
})

test('payload equality reports every missing, extra, and differing relative path without hashes', () => {
  assert.doesNotThrow(() => equalPayload({ 'dir/app.exe': 'same' }, { 'dir/app.exe': 'same' }))
  assert.throws(() => equalPayload({}, {}), /payload does not match/)
  assert.throws(() => equalPayload({ 'dir/missing.dll': 'hash1', 'app.exe': 'hash2' }, { 'extra.txt': 'hash3', 'app.exe': 'hash4' }), error => {
    assert.match(error.message, /"missing":\["dir\/missing.dll"\]/)
    assert.match(error.message, /"extra":\["extra.txt"\]/)
    assert.match(error.message, /"differing":\["app.exe"\]/)
    assert.equal(error.message.includes('hash'), false)
    return true
  })
})

for (const failure of ['msi-admin-extract', 'locate-executable', 'authenticode', 'compare']) {
  test(`the Windows verifier names the ${failure} failure without waiving equality`, () => temporary(root => {
    const installed = path.join(root, 'installed')
    fs.mkdirSync(installed)
    fs.writeFileSync(path.join(installed, 'muniment-desktop.exe'), 'installed')
    const phases = []
    const execute = command => {
      if (command === 'msiexec.exe') {
        if (failure === 'msi-admin-extract') throw nativeFailure(command, { status: 1603, stderr: lease.access })
        if (failure !== 'locate-executable') fs.writeFileSync(path.join(root, 'expanded/muniment-desktop.exe'), 'expected')
      } else if (failure === 'authenticode') throw nativeFailure(command, { status: 1, stdout: 'NotTrusted' })
    }
    assert.throws(() => verifyInstalled({ platform: 'windows', asset: 'muniment_1.0.0_x64_en-US.msi' },
      'candidate.msi', path.join(installed, 'muniment-desktop.exe'), root, { TMPDIR: root }, step => phases.push(step), execute), error => {
      assert.equal(phases.at(-1), `payload/${failure}`)
      assertSafe(redact(error.message))
      if (failure === 'compare') assert.match(error.message, /"differing":\["muniment-desktop.exe"\]/)
      if (failure === 'authenticode') assert.match(error.message, /NotTrusted/)
      return true
    })
  }))
}

test('probe waits report timeout, process exit, and the completed result', async () => {
  let clock = 0
  await assert.rejects(awaitProbeResult(() => false, () => true,
    { now: () => clock, wait: async ms => { clock += ms }, timeout: 500 }), /timed out/)
  assert.equal(clock, 500)
  await assert.rejects(awaitProbeResult(() => false, () => false), /process exited/)
  assert.deepEqual(await awaitProbeResult(() => ({ passed: false }), () => false), { passed: false })
  assert.equal(processStatus(undefined), 'not started')
  assert.equal(processStatus({}), 'spawn failed')
  assert.match(processStatus({ pid: 42, exitCode: null, signalCode: 'SIGTERM' }), /signal=SIGTERM/)
  assert.match(processStatus({ pid: 42, exitCode: 17, signalCode: null }), /exit=17/)
})

test('profile logs retain redacted app and runtime tails without credential files', () => temporary(root => {
  const env = isolatedEnvironment(root, {})
  fs.writeFileSync(path.join(env.TMPDIR, 'subscription-app.log'), `app failed ${lease.access}\n`)
  fs.writeFileSync(path.join(env.TMPDIR, 'subscription-runtime.log'), `runtime failed ${lease.account_id}\n`)
  fs.mkdirSync(path.join(env.MUNIMENT_STATE_DIR, 'browser'))
  fs.writeFileSync(path.join(env.MUNIMENT_STATE_DIR, 'browser/cef.log'), 'CEF failed\nprivate-access-val')
  fs.writeFileSync(path.join(env.PI_CODING_AGENT_DIR, 'auth.json'), 'Do not collect this file.')
  const logs = profileLogs(env, redact)
  assertSafe(logs)
  for (const message of ['app failed', 'runtime failed', 'CEF failed']) assert.ok(logs.includes(message))
  assert.equal(logs.includes('Do not collect'), false)
  assert.equal(logs.includes('private-access-val'), false)
  assert.match(logs, /final incomplete line/)
  assert.match(linuxRuntimeStatus(env), /No runtime identity/)
  fs.mkdirSync(path.join(env.XDG_RUNTIME_DIR, 'muniment'))
  for (const pid of [0, -1, 2147483648, '../other', null]) {
    fs.writeFileSync(path.join(env.XDG_RUNTIME_DIR, 'muniment/desktop-runtime.json'), JSON.stringify({ pid, started: 1 }))
    assert.match(linuxRuntimeStatus(env), /invalid/)
  }
  if (process.platform === 'linux') {
    const stat = fs.readFileSync(`/proc/${process.pid}/stat`, 'utf8')
    const started = Number(stat.slice(stat.lastIndexOf(')') + 2).split(/\s+/)[19])
    const file = path.join(env.XDG_RUNTIME_DIR, 'muniment/desktop-runtime.json')
    fs.writeFileSync(file, JSON.stringify({ pid: process.pid, started }))
    assert.match(linuxRuntimeStatus(env), /exit=none signal=none/)
    fs.writeFileSync(file, JSON.stringify({ pid: process.pid, started: started + 1 }))
    assert.match(linuxRuntimeStatus(env), /stale/)
  }
}))

test('Linux diagnostics retain exit and signal receipts after the desktop reaps the runtime', () => temporary(root => {
  const env = isolatedEnvironment(root, {})
  const directory = path.join(env.XDG_RUNTIME_DIR, 'muniment')
  fs.mkdirSync(directory)
  const identity = { pid: 2147483647, started: 123 }
  const file = path.join(directory, 'desktop-runtime.json')
  const receipt = path.join(directory, `desktop-runtime-${identity.pid}-${identity.started}.exit`)
  fs.writeFileSync(file, JSON.stringify(identity))
  for (const [raw, expected] of [[42 << 8, 'exit=42 signal=none'], [15, 'exit=none signal=15'], [0, 'exit=0 signal=none'], [139, 'exit=none signal=11']]) {
    fs.writeFileSync(receipt, `${raw}\n`)
    assert.ok(linuxRuntimeStatus(env).endsWith(expected))
  }
  for (const invalid of ['', '10752', '-1\n', '65536\n', '127\n', '128\n', '65\n', '257\n', '1.5\n', 'secret\n']) {
    fs.writeFileSync(receipt, invalid)
    assert.match(linuxRuntimeStatus(env), /state=absent exit=unavailable/)
  }
  fs.writeFileSync(receipt, '10752\n')
  fs.writeFileSync(file, JSON.stringify({ ...identity, started: identity.started + 1 }))
  assert.match(linuxRuntimeStatus(env), /state=absent exit=unavailable/)
  fs.writeFileSync(file, JSON.stringify(identity))
  fs.rmSync(receipt)
  if (process.platform !== 'win32') {
    fs.symlinkSync(file, receipt)
    assert.match(linuxRuntimeStatus(env), /state=absent exit=unavailable/)
  }
}))

test('update timeouts read the latest checkpoint without changing the failing step', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-checkpoint-'))
  try {
    const env = isolatedEnvironment(root, {})
    const checkpoint = path.join(env.MUNIMENT_STATE_DIR, 'subscription-probe.json')
    const resultFile = path.join(env.MUNIMENT_STATE_DIR, 'subscription-probe-result.json')
    fs.writeFileSync(checkpoint, JSON.stringify({ phase: 'update' }))
    let clock = 0
    await assert.rejects(awaitUpdateResult(() => fs.existsSync(resultFile), {
      now: () => clock, timeout: 500, wait: async ms => {
        clock += ms
        fs.writeFileSync(checkpoint, JSON.stringify({ phase: 'update-restart' }))
      },
    }), error => {
      writeBlocked(root, sourceSha, 'linux', 'The updated app did not restart.',
        `${probeProgress(env, 'update/wait-result', 'update')}\nerror=${error.message}`, redact)
      return /did not restart/.test(error.message)
    })
    const log = fs.readFileSync(path.join(root, 'linux-subscription.log'), 'utf8')
    assert.match(log, /step=update\/wait-result\nphase=update-restart\nerror=The updated app did not restart/)
    assert.equal(fs.existsSync(resultFile), false)
    for (const content of ['', '{', JSON.stringify({ phase: lease.access }), '{}', 'null']) {
      fs.writeFileSync(checkpoint, content)
      assert.equal(probeProgress(env, 'update/wait-result', 'update'), 'step=update/wait-result\nphase=update')
    }
    fs.rmSync(checkpoint)
    assert.equal(probeProgress(env, 'chat/app-start', 'chat'), 'step=chat/app-start\nphase=chat')
    assert.equal(probeProgress(undefined, 'prerequisites', 'not started'), 'step=prerequisites\nphase=not started')
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('native log tails decode Windows MSI logs, bound large files, and reject links', () => temporary(root => {
  const file = path.join(root, 'msi.log')
  fs.writeFileSync(file, Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(`MSI error 1925 ${lease.access}\r\n`, 'utf16le')]))
  assert.match(readDiagnosticLog(file, root, redact), /MSI error 1925/)
  assertSafe(readDiagnosticLog(file, root, redact))
  fs.writeFileSync(file, 'x'.repeat(100_000) + lease.access + '\n' + `MSI final error ${lease.access}\n`)
  const tail = readDiagnosticLog(file, root, redact)
  assert.match(tail, /MSI final error/)
  assert.ok(tail.length <= 16_385)
  assertSafe(tail)
  fs.writeFileSync(file, 'x'.repeat(100_000) + lease.access + '\n' + 'y'.repeat(65_520) + '\nfinal\n')
  assert.equal(readDiagnosticLog(file, root, redact).includes('value'), false)
  fs.writeFileSync(file, 'x'.repeat(100_000) + lease.access)
  assert.equal(readDiagnosticLog(file, root, redact).includes('access-value'), false)
  fs.rmSync(file)
  if (process.platform !== 'win32') {
    fs.symlinkSync('/etc/passwd', file)
    assert.match(readDiagnosticLog(file, root, redact), /leaves the disposable profile/)
  }
}))

test('blocked logs survive cleanup and collection without stale screenshots or secrets', () => temporary(root => {
  const input = path.join(root, 'input')
  fs.mkdirSync(input)
  fs.writeFileSync(path.join(input, 'screenshot-linux-subscriptions.png'), 'stale')
  writeBlocked(input, sourceSha, 'linux', 'The installed probe did not finish.', `step=chat/wait-result\nphase=chat\n${lease.access}`, redact)
  writeBlocked(input, sourceSha, 'linux', 'The native probe cleanup failed.', `step=cleanup/stop\n${lease.account_id}`, redact)
  const output = path.join(root, 'output')
  assert.equal(collect(sourceSha, output, [input]), 1)
  const log = fs.readFileSync(path.join(output, 'linux-subscription.log'), 'utf8')
  assert.match(log, /step=chat\/wait-result/)
  assert.match(log, /phase=chat/)
  assert.match(log, /step=cleanup\/stop/)
  assertSafe(log)
  assert.equal(fs.existsSync(path.join(input, 'screenshot-linux-subscriptions.png')), false)
  assert.ok(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases.every(item => item.status === 'blocked'))
}))

for (const mode of ['missing-envelope', 'blocked-guest', 'ssh-throw']) {
  test(`the host preserves redacted native diagnostics after ${mode}`, () => temporary(output => {
    fs.writeFileSync(path.join(output, 'linux-subscription.log'), 'stale transcript')
    const status = host({ sourceSha, platform: 'linux', output, leases, models, token, sshKey: key, knownHosts: 'host',
      invoke: options => runDesktopCi({ ...options, spawnProcess(command, args) {
        if (command === 'ssh') {
          if (mode === 'ssh-throw') throw new Error(`SSH failed ${token}`)
          return { status: 1, stdout: `native failure ${privateValues.join('\n')}\n`, stderr: 'runtime exit=42\n' }
        }
        if (mode === 'blocked-guest') {
          writeBlocked(args[2], sourceSha, 'linux', 'The installed probe did not finish.', 'step=chat/wait-result\nphase=chat\napp: exit=42', redact)
        }
        return { status: mode === 'blocked-guest' ? 0 : 1, stderr: 'extract complete' }
      } }),
    })
    assert.equal(status, 1)
    const log = fs.readFileSync(path.join(output, 'linux-subscription.log'), 'utf8')
    assertSafe(log)
    assert.match(log, /step=desktop-ci\/ssh/)
    if (mode === 'blocked-guest') assert.match(log, /step=chat\/wait-result/)
    if (mode !== 'ssh-throw') assert.match(log, /runtime exit=42/)
    assert.equal(log.includes('stale transcript'), false)
    assert.ok(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases.every(item => item.status === 'blocked'))
  }))
}

test('SSH diagnostic tails exclude artifact and screenshot envelopes', () => {
  const transcript = 'native failure\n-----DESKTOP-CI-ARTIFACTS-BEGIN-----\nsecret archive\n-----DESKTOP-CI-ARTIFACTS-END-----\n' +
    '-----DESKTOP-CI-SCREENDUMP-BEGIN-----\nsecret pixels\n-----DESKTOP-CI-SCREENDUMP-END-----\nexit=1'
  assert.equal(transcriptText(transcript), 'native failure\nexit=1')
})

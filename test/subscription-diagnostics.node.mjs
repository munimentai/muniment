import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { transcriptTail } from './e2e/support/transcript-tail.mjs'
import { hostedArm64 } from './e2e/runner/subscription-macos-arm64.mjs'
import { platforms } from './e2e/support/subscription-acceptance.mjs'
import { subscriptionRedactor, diagnosticTail, nativeFailure, processStatus, profileLogs, transcriptText, linuxRuntimeStatus, readDiagnosticLog, probeProgress, payloadDifferenceDetail, reportPayloadDifferences, reportSubscriptionFailure } from './e2e/support/subscription-diagnostics.mjs'
import { awaitProbeResult, awaitUpdateResult, equalPayload, isolatedEnvironment, verifyInstalled, writeBlocked } from './e2e/runner/subscriptions.mjs'
import { host, runDesktopCi } from './e2e/runner/subscription-host.mjs'
import { collect } from './e2e/runner/collect-subscriptions.mjs'
import { legacyPerUserTemplate } from '../.github/lib/windows-upgrade-fixture.mjs'
import { assertMsiPayload } from './windows-msi-payload.mjs'

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

test('payload diagnostic limits do not waive empty payloads or mismatches beyond the limit', () => temporary(root => {
  const summary = path.join(root, 'summary')
  const emitted = []
  const files = Object.fromEntries(Array.from({ length: 51 }, (_, index) => [`file-${index}.dll`, 'same']))
  assert.throws(() => equalPayload(files, { ...files, 'file-50.dll': 'changed' }), /"differing":\["file-50.dll"\]/)
  assert.throws(() => equalPayload({}, {}), error => {
    reportPayloadDifferences(error.message, redact, { emit: text => emitted.push(text), summary })
    return true
  })
  assert.equal(emitted.length, 1)
  assert.equal(emitted[0], 'payload-difference={"missing":[],"extra":[],"differing":[]}')
  assert.match(fs.readFileSync(summary, 'utf8'), /"missing":\[\]/)
}))

test('payload diagnostics cap and redact path lists in the console and step summary', () => temporary(root => {
  const summary = path.join(root, 'summary')
  const emitted = []
  const difference = {
    missing: Array.from({ length: 51 }, (_, index) => `resources/file-${index}.dll`),
    extra: [lease.access, `accounts/${lease.account_id}/state.json`, '/Users/private-person/log',
      'C:\\Users\\private-person\\log', '../private-person/log', 'dir/../../log', 'dir\n::error::injected',
      'x'.repeat(513), 'dir/</pre><script>bad</script>.txt'],
    differing: ['muniment-desktop.exe'],
    contents: 'Do not print file contents.',
  }
  const detail = payloadDifferenceDetail(difference, redact)
  reportPayloadDifferences(`Unrelated log contents.\n${detail}\n${detail}\npayload-difference={\npayload-difference={"missing":42}`, redact,
    { emit: text => emitted.push(text), summary })
  assert.equal(emitted.length, 1)
  const paths = JSON.parse(emitted[0].slice('payload-difference='.length))
  assert.equal(paths.missing.length, 50)
  assert.deepEqual(paths.differing, ['muniment-desktop.exe'])
  assert.equal(paths.extra.filter(value => value === '[invalid path]').length, 5)
  assert.ok(paths.extra.includes('[path exceeds limit]'))
  for (const text of [emitted[0], fs.readFileSync(summary, 'utf8')]) {
    assertSafe(text)
    for (const value of ['file-50', 'private-person', '::error::', 'Do not print', 'Unrelated log']) assert.equal(text.includes(value), false)
  }
  const markdown = fs.readFileSync(summary, 'utf8')
  assert.equal(markdown.includes('<script>'), false)
  assert.match(markdown, /&lt;script&gt;/)
  for (const name of ['missing', 'extra', 'differing']) {
    const capped = JSON.parse(payloadDifferenceDetail({ missing: [], extra: [], differing: [], [name]: difference.missing }).slice('payload-difference='.length))
    assert.equal(capped[name].length, 50)
  }
  assert.equal(payloadDifferenceDetail({ missing: [], extra: [] }), '')
}))

test('payload diagnostics reach the host console and step summary without artifact access', t => temporary(output => {
  const messages = []
  t.mock.method(console, 'error', text => messages.push(text))
  const summary = path.join(output, 'summary')
  const previous = process.env.GITHUB_STEP_SUMMARY
  process.env.GITHUB_STEP_SUMMARY = summary
  try {
    const status = host({ sourceSha, platform: 'windows', output, leases, models, token, sshKey: key, knownHosts: 'host',
      invoke: ({ output: artifacts }) => {
        try { equalPayload({ 'missing.dll': 'hash1', 'app.exe': 'hash2' }, { 'Uninstall muniment.lnk': 'hash3', 'app.exe': 'hash4', [lease.account_id]: 'hash5' }) }
        catch (error) {
          writeBlocked(artifacts, sourceSha, 'windows', 'The installed payload does not match the signed package.',
            `step=payload/compare\nerror=${error.message}`, redact)
        }
        return { status: 1 }
      },
    })
    assert.equal(status, 1)
    for (const text of [messages.join('\n'), fs.readFileSync(summary, 'utf8')]) {
      assert.match(text, /"missing":\["missing.dll"\]/)
      assert.match(text, /"extra":\["Uninstall muniment.lnk","\[REDACTED\]"\]/)
      assert.match(text, /"differing":\["app.exe"\]/)
      assert.equal(text.includes('hash'), false)
      assertSafe(text)
    }
    assert.ok(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases.every(item => item.status === 'blocked'))
  } finally {
    if (previous === undefined) delete process.env.GITHUB_STEP_SUMMARY
    else process.env.GITHUB_STEP_SUMMARY = previous
  }
}))

test('the per-user MSI creates its uninstall shortcut outside the signed payload', () => {
  const template = fs.readFileSync('src-tauri/windows/per-user.wxs', 'utf8')
  const shortcut = template.match(/<Shortcut Id="UninstallShortcut"[\s\S]*?\/>/)[0]
  assert.match(shortcut, /Directory="ApplicationProgramsFolder"/)
  assert.match(shortcut, /Target="\[System64Folder\]msiexec.exe"/)
  assert.match(shortcut, /Arguments="\/x \[ProductCode\]"/)
  const component = template.match(/<Component Id="CMP_UninstallShortcut"[\s\S]*?<\/Component>/)[0]
  assert.match(component, /<RemoveFile Id="LegacyUninstallShortcut" Directory="INSTALLDIR" Name="Uninstall {{product_name}}\.lnk" On="install" \/>/)
})

test('the native upgrade fixture keeps the old per-user shortcut and upgrade condition', () => {
  const template = fs.readFileSync('src-tauri/windows/per-user.wxs', 'utf8')
  const legacy = legacyPerUserTemplate(template)
  assert.match(legacy, /InstallScope="perUser"/)
  assert.match(legacy, /<Shortcut Id="UninstallShortcut"\s+Directory="INSTALLDIR"/)
  assert.match(legacy, /<RemoveShortcuts>Installed AND NOT UPGRADINGPRODUCTCODE<\/RemoveShortcuts>/)
  assert.match(legacy, /<MajorUpgrade Schedule="afterInstallInitialize"[^>]+AllowSameVersionUpgrades="yes"/)
  assert.equal(legacy.includes('<RemoveFile'), false)
  for (const invalid of [legacy, template + template, template.replace('NOT UPGRADINGPRODUCTCODE', '1')]) {
    assert.throws(() => legacyPerUserTemplate(invalid), /legacy per-user fixture requires/)
  }
})

test('the native MSI payload check rejects legacy shortcuts and changed, missing, or extra files', () => temporary(root => {
  const expanded = path.join(root, 'expanded')
  const installed = path.join(root, 'installed')
  const payload = path.join(expanded, 'SourceDir/muniment')
  for (const directory of [payload, installed]) {
    fs.mkdirSync(path.join(directory, 'locales'), { recursive: true })
    fs.writeFileSync(path.join(directory, 'muniment-desktop.exe'), 'desktop')
    fs.writeFileSync(path.join(directory, 'locales/en-US.pak'), 'locale')
  }
  fs.writeFileSync(path.join(expanded, 'candidate.msi'), 'admin database')
  assertMsiPayload(expanded, installed)
  for (const name of ['Uninstall muniment.lnk', 'Uninstall other.lnk', 'runtime.log']) {
    fs.writeFileSync(path.join(installed, name), 'extra')
    assert.throws(() => assertMsiPayload(expanded, installed), /"extra":\[.+\]/)
    fs.rmSync(path.join(installed, name))
  }
  fs.writeFileSync(path.join(installed, 'locales/en-US.pak'), 'changed')
  assert.throws(() => assertMsiPayload(expanded, installed), /"differing":\["locales\/en-US.pak"\]/)
  fs.rmSync(path.join(installed, 'locales/en-US.pak'))
  assert.throws(() => assertMsiPayload(expanded, installed), /"missing":\["locales\/en-US.pak"\]/)
  fs.writeFileSync(path.join(installed, 'locales/en-US.pak'), 'locale')
  assertMsiPayload(expanded, installed)
  fs.writeFileSync(path.join(expanded, 'muniment-desktop.exe'), 'duplicate')
  assert.throws(() => assertMsiPayload(expanded, installed), /exactly one desktop executable/)
  fs.rmSync(path.join(expanded, 'muniment-desktop.exe'))
  fs.rmSync(path.join(payload, 'muniment-desktop.exe'))
  assert.throws(() => assertMsiPayload(expanded, installed), /exactly one desktop executable/)
}))

for (const prefix of ['muniment', 'SourceDir/muniment', 'SourceDir/PFiles/muniment', 'LocalAppDataFolder/muniment']) {
  test(`the Windows verifier compares every file beneath ${prefix}`, () => temporary(root => {
    const installed = path.join(root, 'installed')
    const files = { 'muniment-desktop.exe': 'desktop', 'muniment-runtime.exe': 'runtime', 'locales/en-US.pak': 'locale', 'resources/sidecar.js': 'sidecar' }
    const populate = directory => {
      for (const [name, bytes] of Object.entries(files)) {
        fs.mkdirSync(path.dirname(path.join(directory, name)), { recursive: true })
        fs.writeFileSync(path.join(directory, name), bytes)
      }
    }
    populate(installed)
    const phases = []
    const commands = []
    const verify = verifyInstalled({ platform: 'windows', asset: 'muniment_1.0.0_x64_en-US.msi' },
      'candidate.msi', path.join(installed, 'muniment-desktop.exe'), root, { TMPDIR: root }, step => phases.push(step), command => {
        commands.push(command)
        if (command === 'msiexec.exe') {
          populate(path.join(root, 'expanded', prefix))
          fs.writeFileSync(path.join(root, 'expanded/candidate.msi'), 'admin database')
        }
      })
    assert.deepEqual(commands, ['msiexec.exe', 'powershell.exe'])
    assert.ok(phases.includes('payload/authenticode'))
    verify()
    for (const [name, bytes] of Object.entries(files)) {
      fs.writeFileSync(path.join(installed, name), 'changed')
      assert.throws(verify, /"differing":\[.+\]/)
      fs.writeFileSync(path.join(installed, name), bytes)
      fs.rmSync(path.join(installed, name))
      assert.throws(verify, /"missing":\[.+\]/)
      fs.writeFileSync(path.join(installed, name), bytes)
    }
    for (const name of ['Uninstall muniment.lnk', 'runtime.log']) {
      fs.writeFileSync(path.join(installed, name), 'installer or runtime state')
      assert.throws(verify, /"extra":\[.+\]/)
      fs.rmSync(path.join(installed, name))
    }
    verify()
  }))
}

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
  const env = isolatedEnvironment(root, {}, 'linux')
  fs.writeFileSync(path.join(env.TMPDIR, 'subscription-app.log'), `app failed ${lease.access}\n`)
  fs.writeFileSync(path.join(env.TMPDIR, 'subscription-runtime.log'), `runtime failed ${lease.account_id}\n`)
  fs.mkdirSync(path.join(env.MUNIMENT_STATE_DIR, 'browser'))
  fs.writeFileSync(path.join(env.MUNIMENT_STATE_DIR, 'browser/cef.log'), 'CEF failed\nprivate-access-val')
  fs.writeFileSync(path.join(env.MUNIMENT_STATE_DIR, 'browser/keychain-audit.log'), 'own-key-preflight status=-25308\n')
  fs.writeFileSync(path.join(env.PI_CODING_AGENT_DIR, 'auth.json'), 'Do not collect this file.')
  const logs = profileLogs(env, redact)
  assertSafe(logs)
  for (const message of ['app failed', 'runtime failed', 'CEF failed', 'own-key-preflight status=-25308']) assert.ok(logs.includes(message))
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
  const env = isolatedEnvironment(root, {}, 'linux')
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
    const env = isolatedEnvironment(root, {}, 'linux')
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
  test(`the host preserves redacted native diagnostics after ${mode}`, t => temporary(output => {
    const messages = []
    t.mock.method(console, 'error', text => messages.push(text))
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
    const printed = messages.join('\n')
    assert.match(printed, /platform=linux status=blocked/)
    assert.match(printed, /reason=/)
    assert.match(printed, /step=desktop-ci\/ssh/)
    if (mode === 'blocked-guest') {
      assert.match(printed, /reason="The installed probe did not finish\."/)
      assert.match(printed, /step=chat\/wait-result/)
    }
    if (mode !== 'ssh-throw') assert.match(printed, /runtime exit=42/)
    assertSafe(printed)
    assert.equal(printed.includes('stale transcript'), false)
    assert.ok(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases.every(item => item.status === 'blocked'))
  }))
}

test('SSH diagnostic tails exclude artifact and screenshot envelopes', () => {
  const transcript = 'native failure\n-----DESKTOP-CI-ARTIFACTS-BEGIN-----\nsecret archive\n-----DESKTOP-CI-ARTIFACTS-END-----\n' +
    '-----DESKTOP-CI-SCREENDUMP-BEGIN-----\nsecret pixels\n-----DESKTOP-CI-SCREENDUMP-END-----\nexit=1'
  assert.equal(transcriptText(transcript), 'native failure\nexit=1')
})

for (const platform of ['linux', 'windows', 'macos-x64']) {
  test(`the ${platform} host prints a bounded summary for a nonzero guest result`, t => temporary(output => {
    const messages = []
    t.mock.method(console, 'error', text => messages.push(text))
    assert.equal(host({ sourceSha, platform, output, leases, models, token, sshKey: key, knownHosts: 'host',
      invoke: ({ output: artifacts }) => {
        writeBlocked(artifacts, sourceSha, platform, `The quota check failed. ${privateValues.join(' ')}`,
          'old diagnostic\n' + 'x.'.repeat(15_000) + `\nstep=chat/wait-result\nquota exhausted ${privateValues.join('\n')}`)
        return { status: 1 }
      },
    }), 1)
    const printed = messages.join('\n')
    assert.ok(printed.includes(`platform=${platform} status=blocked`))
    assert.match(printed, /reason="The quota check failed\./)
    assert.match(printed, /step=chat\/wait-result/)
    assert.match(printed, /quota exhausted/)
    assert.equal(printed.includes('old diagnostic'), false)
    assert.ok(printed.length < 20_000)
    assertSafe(printed)
  }))
}

for (const mode of ['blocked', 'throw', 'preflight']) {
  test(`the ARM64 job reports ${mode} failures after the guest settles`, async t => {
    const output = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-arm-diagnostics-'))
    const messages = []
    t.mock.method(console, 'error', text => messages.push(text))
    const previousKey = process.env.DESKTOP_CI_SSH_KEY
    process.env.DESKTOP_CI_SSH_KEY = key
    try {
      const status = await hostedArm64({ sourceSha, output, leases, models, token,
        runtime: 'darwin', arch: 'arm64', uid: 501,
        env: { RUNNER_ARCH: 'ARM64', RUNNER_OS: 'macOS', GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' },
        execute: command => ({ status: mode === 'preflight' ? 1 : 0,
          stdout: command.endsWith('uname') ? 'arm64' : command.endsWith('stat') ? '501' : '' }),
        invoke: async () => {
          await Promise.resolve()
          if (mode === 'throw') throw new Error(`Native failure ${privateValues.join(' ')}`)
          writeBlocked(output, sourceSha, 'macos-arm64', 'The subscription authentication failed.',
            `step=chat/wait-result\nauth failed ${privateValues.join(' ')}`)
          return 1
        },
      })
      assert.equal(status, 1)
      const printed = messages.join('\n')
      assert.match(printed, /platform=macos-arm64 status=blocked/)
      if (mode === 'blocked') {
        assert.match(printed, /reason="The subscription authentication failed\."/)
        assert.match(printed, /step=chat\/wait-result/)
      } else assert.match(printed, /step=hosted-arm64/)
      assertSafe(printed)
    } finally {
      fs.rmSync(output, { recursive: true, force: true })
      if (previousKey === undefined) delete process.env.DESKTOP_CI_SSH_KEY
      else process.env.DESKTOP_CI_SSH_KEY = previousKey
    }
  })
}

test('failure summaries handle missing, malformed, contradictory, and partial evidence', t => temporary(output => {
  const messages = []
  t.mock.method(console, 'error', text => messages.push(text))
  const evidenceFile = path.join(output, 'linux-subscription.json')
  const proofFile = path.join(output, 'release-acceptance.json')
  for (const proof of [undefined, '{', 'null', '{"cases":[]}', '{"cases":[null]}']) {
    messages.length = 0
    if (proof !== undefined) fs.writeFileSync(proofFile, proof)
    fs.writeFileSync(evidenceFile, '{"status":"passed"}')
    reportSubscriptionFailure(output, 'linux', 1, redact)
    assert.match(messages.join('\n'), /platform=linux status=blocked/)
    assert.match(messages.join('\n'), /No diagnostic log exists/)
  }
  messages.length = 0
  fs.writeFileSync(proofFile, JSON.stringify({ cases: [
    { platform: 'linux', status: 'passed' },
    { platform: 'linux', status: 'blocked', reason: 'The installed feature check did not finish.' },
  ] }))
  reportSubscriptionFailure(output, 'linux', 1, redact)
  assert.match(messages.join('\n'), /reason="The installed feature check did not finish\."/)
  messages.length = 0
  fs.writeFileSync(proofFile, JSON.stringify({ cases: [{ platform: 'linux', status: 'passed' }] }))
  reportSubscriptionFailure(output, 'linux', 0, redact)
  assert.deepEqual(messages, [])
  reportSubscriptionFailure(output, 'linux', 1, redact)
  assert.match(messages.join('\n'), /platform=linux status=blocked/)
  messages.length = 0
  fs.writeFileSync(evidenceFile, JSON.stringify({ status: 'blocked', reason: 'old reason '.repeat(2000) + lease.access + '\nFinal reason.' }))
  reportSubscriptionFailure(output, 'linux', 1, redact)
  assert.match(messages[0], /Final reason\./)
  assert.ok(messages[0].length < 2200)
  assertSafe(messages.join('\n'))
}))

test('collection prints every platform reason with lease-aware redaction', t => temporary(root => {
  const messages = []
  t.mock.method(console, 'error', text => messages.push(text))
  const previous = process.env.FACTORY_SUBSCRIPTION_LEASES
  process.env.FACTORY_SUBSCRIPTION_LEASES = leases
  try {
    const inputs = platforms.slice(0, 3).map(platform => {
      const input = path.join(root, platform)
      writeBlocked(input, sourceSha, platform, `The ${platform} subscription failed. ${lease.access} ${lease.account_id} ${leases}`)
      return input
    })
    assert.equal(collect(sourceSha, path.join(root, 'out'), inputs), 1)
    assert.equal(messages.length, 4)
    for (const [index, platform] of platforms.entries()) {
      assert.ok(messages[index].includes(`platform=${platform} status=blocked`))
      assert.match(messages[index], /reason="/)
      if (index < 3) assert.ok(messages[index].includes(`The ${platform} subscription failed.`))
    }
    assertSafe(messages.join('\n'))
  } finally {
    if (previous === undefined) delete process.env.FACTORY_SUBSCRIPTION_LEASES
    else process.env.FACTORY_SUBSCRIPTION_LEASES = previous
  }
}))

test('transcript tails remove long, repeated, and incomplete envelopes before line and size limits', () => {
  const envelope = name => `-----DESKTOP-CI-${name}-BEGIN-----\r\n` +
    'encoded pixels\r\n'.repeat(400) + `-----DESKTOP-CI-${name}-END-----\r\n`
  const text = 'old line\n' + 'progress\n'.repeat(201) + privateValues.join('\n') + '\nNative failure: exit=42\n' +
    envelope('ARTIFACTS') + envelope('SCREENDUMP') + envelope('SCREENDUMP') + 'Final status=1\n'
  const tail = transcriptTail(text, redact)
  assert.match(tail, /Native failure: exit=42/)
  assert.match(tail, /Final status=1/)
  for (const hidden of ['old line', 'encoded pixels', 'DESKTOP-CI']) assert.equal(tail.includes(hidden), false)
  assert.ok(tail.split('\n').length <= 200)
  assertSafe(tail)
  assert.equal(transcriptTail('error\n-----DESKTOP-CI-SCREENDUMP-BEGIN-----\npixels', redact), 'error')
  assert.equal(transcriptTail('', redact), '')
  assert.equal(transcriptTail('x.'.repeat(15_000) + '\nlast error', redact).length, 16_384)
  const multilineKey = '-----BEGIN OPENSSH PRIVATE KEY-----\n' + 'a'.repeat(150) + '\n-----END OPENSSH PRIVATE KEY-----'
  const safe = transcriptTail(`error\n${multilineKey}\nlast error`, subscriptionRedactor({ values: [multilineKey] }))
  assert.equal(safe.includes('PRIVATE KEY'), false)
  assert.equal(safe.includes('a'.repeat(150)), false)
})

test('nightly uses the redacted transcript tail on every native E2E failure', () => temporary(root => {
  const workflow = fs.readFileSync('.github/workflows/nightly.yml', 'utf8')
  for (const platform of ['linux', 'windows', 'macos']) {
    const job = workflow.split(`  ${platform}-e2e:\n`)[1].split(/\n  [a-z][\w-]+:\n/)[0]
    assert.match(job, /if \(\( run_status != 0 \|\| extract_status != 0 \)\); then\n\s+node test\/e2e\/support\/transcript-tail\.mjs "\$output"/)
    assert.equal(job.includes('tail -n 200'), false)
  }
  const file = path.join(root, 'transcript')
  fs.writeFileSync(file, `Native failure ${privateValues.join('\n')} fixture-user\n` +
    '-----DESKTOP-CI-SCREENDUMP-BEGIN-----\n' + 'pixels\n'.repeat(300) + '-----DESKTOP-CI-SCREENDUMP-END-----\n')
  const result = spawnSync(process.execPath, ['test/e2e/support/transcript-tail.mjs', file], {
    encoding: 'utf8', env: { ...process.env, FACTORY_SUBSCRIPTION_LEASES: leases,
      GH_TOKEN: token, DESKTOP_CI_SSH_KEY: key, FIXTURE_USERNAME: 'fixture-user' },
  })
  assert.equal(result.status, 0)
  assert.match(result.stderr, /Native failure/)
  for (const hidden of ['pixels', 'DESKTOP-CI', 'fixture-user']) assert.equal(result.stderr.includes(hidden), false)
  assertSafe(result.stderr)
  const subscriptions = fs.readFileSync('.github/workflows/subscriptions.yml', 'utf8').split('  collect:\n')[1]
  assert.match(subscriptions, /FACTORY_SUBSCRIPTION_LEASES: \$\{\{ secrets\.FACTORY_SUBSCRIPTION_LEASES \}\}/)
}))

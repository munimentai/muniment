import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { transcriptTail } from './e2e/support/transcript-tail.mjs'
import { hostedArm64 } from './e2e/runner/subscription-macos-arm64.mjs'
import { platforms } from './e2e/support/subscription-acceptance.mjs'
import { subscriptionRedactor, diagnosticTail, nativeFailure, processStatus, profileLogs, transcriptText, linuxRuntimeStatus, readDiagnosticLog, probeProgress, readProbeProgress, probeFailure, payloadDifferenceDetail, reportPayloadDifferences, reportSubscriptionFailure } from './e2e/support/subscription-diagnostics.mjs'
import { awaitProbeResult, awaitUpdateResult, equalPayload, isolatedEnvironment, verifyInstalled, writeBlocked, prepareProbeHome, removeProbeProfile } from './e2e/runner/subscriptions.mjs'
import { host, runDesktopCi } from './e2e/runner/subscription-host.mjs'
import { collect } from './e2e/runner/collect-subscriptions.mjs'
import { legacyPerUserTemplate } from '../.github/lib/windows-upgrade-fixture.mjs'
import { assertMsiPayload } from './windows-msi-payload.mjs'

test('the disposable profile selects Home before the chat composer mounts', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-home-'))
  try {
    prepareProbeHome(root)
    const { location } = JSON.parse(fs.readFileSync(path.join(root, 'home.json'), 'utf8'))
    assert.equal(location, path.join(root, 'probe-home'))
    for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
      assert.ok(fs.statSync(path.join(location, directory)).isDirectory())
    }
    const probe = fs.readFileSync('test/e2e/support/subscription-probe.js', 'utf8')
    assert.ok(probe.includes("document.querySelector('textarea#composer-message')"))
    assert.ok(!probe.includes('textarea[placeholder='))
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('blocked diagnostics name the turn and provider outcome without replies or tokens', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-progress-'))
  const env = { MUNIMENT_STATE_DIR: root }
  const row = { phase: 'chat', stage: 'reply', turn: 2, requested: 'model-two', transport: 'pending', error_class: 'timeout' }
  const progress = path.join(root, 'subscription-probe-progress.jsonl')
  const transports = path.join(root, 'subscription-probe-transport-progress.jsonl')
  try {
    fs.writeFileSync(progress, JSON.stringify({ ...row, reply: 'PRIVATE REPLY', access: 'PRIVATE TOKEN' }) + '\n')
    assert.deepEqual(readProbeProgress(env), [row])
    assert.equal(probeFailure(env), 'product')
    for (const error_class of ['auth', 'quota', 'http', 'network', 'stream']) {
      fs.writeFileSync(transports, JSON.stringify({ ...row, stage: 'transport', transport: 'failed', error_class }) + '\n')
      assert.equal(probeFailure(env), ['auth', 'quota'].includes(error_class) ? error_class : 'product')
      const detail = probeProgress(env, 'chat/wait-result', 'chat')
      assert.match(detail, /"turn":2,"requested":"model-two"/)
      assert.ok(detail.includes(`"error_class":"${error_class}"`))
      assert.ok(!detail.includes('PRIVATE'))
      assert.match(detail.split('\n').at(-1), /^probe-current=.*"stage":"reply"/)
      writeBlocked(root, sourceSha, 'linux', 'The probe did not pass.', detail, value => value, probeFailure(env))
      const evidence = JSON.parse(fs.readFileSync(path.join(root, 'linux-subscription.json'), 'utf8'))
      assert.equal(evidence.status, ['auth', 'quota'].includes(error_class) ? 'blocked' : 'failed')
      assert.equal(evidence.failure_kind, probeFailure(env))
    }
    // Ignore stale refusals, successful retries, malformed rows, and partial writes.
    fs.writeFileSync(transports, JSON.stringify({ ...row, turn: 1, transport: 'failed', error_class: 'auth' }) + '\n')
    assert.equal(probeFailure(env), 'product')
    fs.writeFileSync(transports, JSON.stringify({ ...row, transport: 'complete', error_class: 'none' }) + '\n')
    assert.equal(probeFailure(env), 'product')
    fs.appendFileSync(progress, '{"reply":"PRIVATE')
    assert.deepEqual(readProbeProgress(env), [row])
    for (const change of [{ turn: -1 }, { turn: 4 }, { turn: 0.5 }, { requested: 'bad\nmodel' }, { stage: 'PRIVATE' }, { error_class: 'PRIVATE' }]) {
      fs.writeFileSync(progress, JSON.stringify({ ...row, ...change }) + '\n')
      assert.deepEqual(readProbeProgress(env), [])
    }
    fs.writeFileSync(progress, 'x'.repeat(32_769))
    assert.deepEqual(readProbeProgress(env), [])
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('inventory timeouts keep model save results, OS codes, and saved defaults', () => temporary(root => {
  const env = { MUNIMENT_STATE_DIR: root }
  const file = path.join(root, 'subscription-probe-progress.jsonl')
  const base = { phase: 'chat', turn: 1, requested: 'terra', error_class: 'none' }
  const inventory = { ...base, stage: 'inventory', transport: 'not-started', error_class: 'timeout',
    settings_read: true, defaultProvider: 'muniment-router', defaultModel: 'openai-codex/luna' }
  const current = rows => {
    fs.writeFileSync(file, rows.map(row => JSON.stringify(row)).join('\n') + '\n')
    const text = probeProgress(env, 'chat/verify-result', 'chat')
    assert.ok(!text.includes('PRIVATE'))
    return JSON.parse(text.split('\n').at(-1).slice('probe-current='.length))
  }
  for (const [transport, code, rejected] of [['pending', null, null], ['complete', null, false], ['failed', 32, true], ['failed', 1175, true], ['failed', null, true]]) {
    const row = current([{ ...base, stage: 'model-save', transport, os_error: code, message: 'PRIVATE path and token' }, inventory])
    assert.equal(row.model_save_status, transport)
    assert.equal(row.model_save_rejected, rejected)
    assert.equal(row.os_error, code)
    assert.equal(row.defaultProvider, 'muniment-router')
    assert.equal(row.defaultModel, 'openai-codex/luna')
    assert.equal(row.settings_read, true)
    assert.equal(probeFailure(env), 'product')
  }
  const stale = { ...base, stage: 'model-save', transport: 'failed', os_error: 32 }
  for (const change of [{ turn: 0 }, { requested: 'luna' }, { phase: 'features' }]) {
    const row = current([{ ...stale, ...change }, inventory])
    assert.equal(row.model_save_status, 'not-started')
    assert.equal(row.model_save_rejected, null)
    assert.equal(row.os_error, null)
  }
  const row = current([stale, { ...stale, transport: 'complete', os_error: 32 }, inventory])
  assert.equal(row.model_save_rejected, false)
  assert.equal(row.os_error, null)
  for (const code of ['PRIVATE', '32', 1.5, 2147483648, -2147483649, {}]) {
    assert.equal(current([{ ...stale, os_error: code }, inventory]).os_error, null)
  }
  for (const value of ['C:/PRIVATE', 'C:\\PRIVATE', '/PRIVATE', '../PRIVATE', 'PRIVATE\nTOKEN', 'PRIVATE\n', 'PRIVATE\r', {}, 4]) {
    const row = current([{ ...inventory, defaultProvider: value, defaultModel: value }])
    assert.equal(row.defaultProvider, '[redacted]')
    assert.equal(row.defaultModel, '[redacted]')
  }
  const empty = current([{ ...inventory, settings_read: false, defaultProvider: null, defaultModel: null }])
  assert.equal(empty.settings_read, false)
  assert.equal(empty.defaultProvider, null)
  assert.equal(empty.defaultModel, null)
}))

test('Windows artifacts keep redacted transport details at chat/verify-result', t => temporary(root => {
  t.mock.method(console, 'error', () => {})
  const env = { MUNIMENT_STATE_DIR: root }
  const row = { phase: 'chat', stage: 'reply', turn: 0, requested: 'model-one', transport: 'pending', error_class: 'reply-failed' }
  fs.writeFileSync(path.join(root, 'subscription-probe-progress.jsonl'), JSON.stringify(row) + '\n')
  const endpoints = ['dns', 'connect', 'tls', 'tls_certificate', 'proxy', 'timeout', 'other'].map(kind => [kind, 'api.example.com'])
  endpoints.push(['proxy', '127.0.0.1'], ['proxy', 'proxy.example'], ['dns', 'redirect.example'])
  for (const [transport_kind, host] of endpoints) {
    const failure = { ...row, stage: 'transport', transport: 'failed', error_class: 'network', transport_kind, host }
    fs.writeFileSync(path.join(root, 'subscription-probe-transport-progress.jsonl'), JSON.stringify({
      ...failure, url: 'https://user:PRIVATE@api.example.com/PRIVATE?token=PRIVATE',
      headers: { Authorization: 'Bearer PRIVATE', 'Proxy-Authorization': 'Basic dXNlcjpQUklWQVRF' },
      proxy: 'http://user:PRIVATE@proxy.example:80', message: 'PRIVATE error detail',
    }) + '\n')
    assert.deepEqual(readProbeProgress(env, true), [failure])
    assert.equal(probeFailure(env), 'product')
    const detail = probeProgress(env, 'chat/verify-result', 'chat')
    const input = path.join(root, `${transport_kind}-${host}`)
    const output = path.join(root, `${transport_kind}-${host}-collected`)
    writeBlocked(input, sourceSha, 'windows', 'The subscription probe did not pass.', detail, redact, probeFailure(env))
    assert.equal(collect(sourceSha, output, [input]), 1)
    const log = fs.readFileSync(path.join(output, 'windows-subscription.log'), 'utf8')
    assert.match(log, /step=chat\/verify-result/)
    const progress = JSON.parse(log.split('\n').find(line => line.startsWith('provider-progress=')).slice('provider-progress='.length))
    assert.deepEqual(progress, failure)
    assert.ok(log.includes(`"provider_transport_kind":"${transport_kind}"`))
    assert.ok(log.includes(`"provider_host":"${host}"`))
    for (const secret of ['PRIVATE', 'Authorization', 'dXNlcjpQUklWQVRF', 'token=', '://']) assert.ok(!log.includes(secret))
  }
}))

test('transport diagnostics reject malformed metadata without hiding the failure', () => temporary(root => {
  const env = { MUNIMENT_STATE_DIR: root }
  const file = path.join(root, 'subscription-probe-transport-progress.jsonl')
  const row = { phase: 'chat', stage: 'transport', turn: 0, requested: 'model-one', transport: 'failed', error_class: 'network' }
  const read = change => {
    fs.writeFileSync(file, JSON.stringify({ ...row, ...change }) + '\n')
    return readProbeProgress(env, true)[0]
  }
  for (const host of ['api.example.com', 'example.com.', '127.0.0.1', '[::1]']) {
    assert.equal(read({ transport_kind: 'tls', host }).host, host)
  }
  for (const host of [null, '', 42, {}, 'https://example.com?token=PRIVATE', 'user:PRIVATE@example.com',
    'example.com:443', 'example.com/PRIVATE', 'example.com?PRIVATE', 'example.com#PRIVATE',
    'example.com\nAuthorization: PRIVATE', 'example.com..', '-example.com', 'PRIVATE!example.com',
    '[::1]:443', '[xyz]', `${'x'.repeat(64)}.com`, 'x'.repeat(254)]) {
    assert.deepEqual(read({ transport_kind: 'tls', host }), { ...row, transport_kind: 'tls', host: null })
  }
  for (const transport_kind of [null, '', 'PRIVATE', 'TLS', 42, {}]) {
    assert.deepEqual(read({ transport_kind, host: 'api.example.com' }), row)
  }
  assert.deepEqual(read({}), row)
  for (const change of [{ stage: 'reply' }, { transport: 'complete' }, { error_class: 'auth' }]) {
    assert.deepEqual(read({ ...change, transport_kind: 'tls', host: 'api.example.com' }), { ...row, ...change })
  }
}))

test('Windows shutdown waits on descendant handles before bounded profile removal', async () => {
  const { stopWindowsTree } = await import('./e2e/support/subscription-windows-process.mjs')
  for (const identity of [0, -1, 1.5, '123', 2147483648, { pid: 123 }]) {
    await assert.rejects(stopWindowsTree(identity), /identity is missing/)
  }
  let clock = 0, attempts = 0
  await removeProbeProfile('/disposable', { now: () => clock, wait: async ms => { clock += ms }, remove: async (_, options) => {
    assert.deepEqual(options, { recursive: true, force: true })
    if (++attempts < 3) throw Object.assign(new Error('The profile is busy.'), { code: 'EPERM' })
  } })
  assert.equal(attempts, 3)
  assert.equal(clock, 500)
  attempts = 0
  await assert.rejects(removeProbeProfile('/disposable', { timeout: 500, now: () => clock, wait: async ms => { clock += ms }, remove: async () => {
    attempts++
    throw Object.assign(new Error('The profile is busy.'), { code: 'EBUSY' })
  } }), /busy/)
  assert.equal(attempts, 3)
  assert.equal(clock, 1000)
  await assert.rejects(removeProbeProfile('/disposable', { remove: async () => { throw new Error('The disk failed.') } }), /disk failed/)
})

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

test('writer failures keep their classification, reason, and progress through collection and summaries', t => temporary(root => {
  const messages = []
  t.mock.method(console, 'error', text => messages.push(text))
  for (const failureKind of ['product', 'auth', 'quota']) {
    const input = path.join(root, failureKind)
    const output = path.join(root, `${failureKind}-collected`)
    const reason = `The subscription probe stopped with a ${failureKind} failure.`
    const progress = 'probe-current={"phase":"chat","stage":"reply","turn":2,"requested":"model-two","transport":"failed","error_class":"' +
      (failureKind === 'product' ? 'stream' : failureKind) + '"}'
    writeBlocked(input, sourceSha, 'windows', reason, `${progress}\nAuthorization: Bearer private-transport-token`, redact, failureKind)
    assert.equal(collect(sourceSha, output, [input]), 1)
    const evidence = JSON.parse(fs.readFileSync(path.join(output, 'windows-subscription.json')))
    assert.deepEqual(evidence, { status: failureKind === 'product' ? 'failed' : 'blocked', reason, failure_kind: failureKind })
    const proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    assert.ok(proof.cases.filter(item => item.platform === 'windows').every(item => item.reason === reason))
    const log = fs.readFileSync(path.join(output, 'windows-subscription.log'), 'utf8')
    assert.ok(log.includes(progress))
    assert.ok(!log.includes('private-transport-token'))
    assert.ok(messages.some(text => text.includes(`platform=windows status=${evidence.status}`) && text.includes(reason)))
    assert.ok(messages.some(text => text.includes(progress)))
    messages.length = 0
    reportSubscriptionFailure(output, 'windows', 1, redact)
    assert.ok(messages.some(text => text.includes(`platform=windows status=${evidence.status}`)))
    assert.ok(messages.some(text => text.includes(progress)))
    assert.ok(!messages.join('\n').includes('private-transport-token'))
    messages.length = 0
  }
}))

test('the collector rejects contradictory failure classifications', t => temporary(root => {
  t.mock.method(console, 'error', () => {})
  for (const [status, failure_kind] of [['failed', 'auth'], ['failed', undefined], ['blocked', 'product'], ['unknown', 'product']]) {
    const input = path.join(root, 'input')
    const output = path.join(root, 'output')
    writeBlocked(input, sourceSha, 'windows', 'The subscription probe did not finish.')
    fs.writeFileSync(path.join(input, 'windows-subscription.json'), JSON.stringify({ status, failure_kind, reason: 'The subscription probe did not finish.' }))
    assert.equal(collect(sourceSha, output, [input]), 1)
    const evidence = JSON.parse(fs.readFileSync(path.join(output, 'windows-subscription.json')))
    assert.equal(evidence.status, 'blocked')
    assert.match(evidence.reason, /Supply unique evidence/)
  }
}))

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

for (const platform of ['windows', 'macos-arm64', 'macos-x64']) {
  test(`the ${platform} profile reads only redacted named logs from the disposable native home`, () => temporary(root => {
    const profile = path.join(root, 'profile')
    const nativeHome = path.join(root, 'native-home')
    const ownerHome = path.join(root, 'owner-home')
    fs.mkdirSync(nativeHome)
    fs.mkdirSync(ownerHome)
    // Model the verified native home independently of the host platform's path syntax.
    const env = isolatedEnvironment(profile, {}, 'linux')
    const windows = platform === 'windows'
    env[windows ? 'LOCALAPPDATA' : 'HOME'] = nativeHome
    const name = windows ? 'runtime-native' : 'runtime-service'
    const file = path.join(nativeHome, windows ? 'ai.muniment.desktop/logs/runtime.log' : 'Library/Logs/Muniment/runtime-service.log')
    fs.mkdirSync(path.dirname(file), { recursive: true })
    fs.writeFileSync(file, `native startup failed ${privateValues.join(' ')}\naccess_token=unknown-native-secret\n`)
    fs.writeFileSync(path.join(nativeHome, 'auth.json'), 'Do not collect native credentials.\n')
    fs.writeFileSync(path.join(path.dirname(file), 'other.log'), 'Do not collect unnamed logs.\n')
    fs.writeFileSync(path.join(env.TMPDIR, 'subscription-app.log'), `app failed ${lease.access}\n`)
    const logs = profileLogs(env, redact)
    assert.match(logs, new RegExp(`${name} tail:\\nnative startup failed`))
    assert.match(logs, /app failed/)
    assertSafe(logs)
    assert.equal(logs.includes('unknown-native-secret'), false)
    assert.equal(logs.includes('Do not collect'), false)
    assert.equal(logs.includes('leaves the disposable profile'), false)

    fs.rmSync(file)
    fs.mkdirSync(file)
    assert.match(profileLogs(env, redact), new RegExp(`${name} tail:\\nThe log is not a regular file`))
    fs.rmSync(file, { recursive: true })
    assert.match(profileLogs(env, redact), new RegExp(`${name} tail:\\nNo log exists`))
    if (process.platform !== 'win32') {
      const credentials = path.join(nativeHome, 'auth.json')
      fs.symlinkSync(credentials, file)
      assert.match(profileLogs(env, redact), new RegExp(`${name} tail:\\nThe log is not a regular file`))
      fs.rmSync(file)
      fs.writeFileSync(path.join(ownerHome, path.basename(file)), 'Do not collect owner data.\n')
      fs.rmSync(path.dirname(file), { recursive: true })
      fs.symlinkSync(ownerHome, path.dirname(file))
      const escaped = profileLogs(env, redact)
      assert.match(escaped, new RegExp(`${name} tail:\\nThe log leaves the disposable profile`))
      assert.equal(escaped.includes('Do not collect'), false)

      // Native homes do not expand the allowed roots for synthetic profile logs.
      fs.rmSync(path.join(env.TMPDIR, 'subscription-app.log'))
      fs.symlinkSync(credentials, path.join(env.TMPDIR, 'subscription-app.log'))
      assert.match(profileLogs(env, redact), /app tail:\nThe log leaves the disposable profile/)
    }
  }))
}

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

for (const mode of ['failed', 'throw']) {
  test(`Windows update diagnostics survive a ${mode} guest and collection`, t => temporary(root => {
    t.mock.method(console, 'error', () => {})
    const output = path.join(root, 'host'), combined = path.join(root, 'combined')
    const startup = [{ pid: 456, code: 'started' }, { pid: 456, code: 'job-open-failed' }]
    assert.equal(host({ sourceSha, platform: 'windows', output, leases, models, token, sshKey: key, knownHosts: 'host',
      invoke: ({ output: artifacts }) => {
        writeBlocked(artifacts, sourceSha, 'windows', 'The updated app did not restart.')
        fs.writeFileSync(path.join(artifacts, 'windows-subscription-relaunch.json'), JSON.stringify({ parent_pid: 123,
          startup, processes: [{ pid: 456, observed: true, exit_code: 1, cleanup: false }], secret: privateValues.join(' ') }) + '\n')
        fs.writeFileSync(path.join(artifacts, 'windows-subscription-msi.log'), 'action=LaunchApplication state=ended result=1\nmsiexec_result=0\n')
        if (mode === 'throw') throw new Error('The native transport failed.')
        return { status: 1 }
      },
    }), 1)
    assert.equal(collect(sourceSha, combined, [output]), 1)
    const result = JSON.parse(fs.readFileSync(path.join(combined, 'windows-subscription-relaunch.json')))
    assert.deepEqual(result.startup, startup)
    assert.equal(result.relaunch_started, true)
    assert.equal(result.processes[0].exit_code, 1)
    assertSafe(JSON.stringify(result))
    assert.equal(fs.readFileSync(path.join(combined, 'windows-subscription-msi.log'), 'utf8'), 'action=LaunchApplication state=ended result=1\nmsiexec_result=0\n')
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

test('The failure summary names failed features after a successful update restart.', t => temporary(output => {
  const messages = []
  t.mock.method(console, 'error', text => messages.push(text))
  const platform = 'macos-arm64'
  const evidenceFile = path.join(output, `${platform}-subscription.json`)
  fs.writeFileSync(evidenceFile, JSON.stringify({ status: 'passed' }))
  const reason = 'The installed feature check did not finish. Read the native runner log.'
  // Match the saved native run without its profile paths or provider replies.
  const cases = [
    { platform, feature: 'chat', status: 'passed' },
    { platform, feature: 'account-balancing', status: 'blocked', reason, failure_stage: 'check', error_class: 'check-failed' },
    { platform, feature: 'mcp', status: 'blocked', reason, failure_stage: 'tool-turn', error_class: 'reply-phase' },
    { platform, feature: 'restart-persistence', status: 'passed' },
    { platform, feature: 'signed-update', status: 'passed' },
  ]
  const proofFile = path.join(output, 'release-acceptance.json')
  fs.writeFileSync(proofFile, JSON.stringify({ cases }))
  fs.writeFileSync(path.join(output, `${platform}-subscription.log`),
    'account-balancing: blocked\nmcp: blocked\n' + 'runtime log\n'.repeat(2000) +
    'muniment-runtime: macos peer read failed operation=LOCAL_PEERPID\n' +
    'muniment-runtime: exit status=75 cause=upgrade refresh\nphase=update-restart\n')
  const summary = 'reason="The installed feature checks failed: account-balancing (check/check-failed), mcp (tool-turn/reply-phase)."'
  for (const status of [0, 1]) {
    messages.length = 0
    reportSubscriptionFailure(output, platform, status, redact)
    assert.equal(messages[0], `platform=${platform} status=blocked\n${summary}`)
    assert.match(messages[1], /phase=update-restart/)
    assert.ok(messages[1].length < 20_000)
    assert.equal(messages[1].includes('account-balancing'), false)
  }

  // Reject unknown codes, other platforms, and stale codes on passing cases.
  cases.push({ platform, feature: 'files', status: 'blocked', failure_stage: 'check', error_class: 'private detail' },
    { platform: 'linux', feature: 'tools', status: 'blocked', failure_stage: 'check', error_class: 'timeout' })
  Object.assign(cases.at(4), { failure_stage: 'restore', error_class: 'check-failed' })
  fs.writeFileSync(proofFile, JSON.stringify({ cases }))
  messages.length = 0
  reportSubscriptionFailure(output, platform, 1, redact)
  assert.equal(messages[0], `platform=${platform} status=blocked\n${summary}`)

  // An incomplete run keeps its own failure instead of a prior feature failure.
  for (const status of ['blocked', 'failed']) {
    const reason = 'The installed update failed or did not restore the disposable profile after relaunch.'
    fs.writeFileSync(evidenceFile, JSON.stringify({ status, reason }))
    messages.length = 0
    reportSubscriptionFailure(output, platform, 1, redact)
    assert.equal(messages[0], `platform=${platform} status=${status}\nreason=${JSON.stringify(reason)}`)
  }
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

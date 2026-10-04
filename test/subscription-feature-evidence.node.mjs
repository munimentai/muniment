import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import vm from 'node:vm'
import { EventEmitter } from 'node:events'
import * as runner from './e2e/runner/subscriptions.mjs'
import * as acceptance from './e2e/support/subscription-acceptance.mjs'
import * as diagnostics from './e2e/support/subscription-diagnostics.mjs'
import { collect } from './e2e/runner/collect-subscriptions.mjs'

const sourceSha = 'a'.repeat(40)
const platform = 'linux'
const read = file => JSON.parse(fs.readFileSync(file, 'utf8'))
const save = (file, value) => fs.writeFileSync(file, JSON.stringify(value), { mode: 0o600 })
const mcpFailure = ['failed', 'connection-test', 'timeout']
const artifactFailure = ['failed', 'save', 'check-failed']

async function scenario(t, { featureFailures = {}, failAt, updateFailure = false, screenshotError } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-evidence-'))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const output = path.join(root, 'output')
  const packageFile = path.join(root, 'package')
  fs.writeFileSync(packageFile, 'candidate')
  const models = ['one', 'two', 'three', 'four'].map(id => ({ family: 'openai', id }))
  const packageName = 'muniment_1.0.0_amd64.AppImage'
  const candidateFile = path.join(root, 'candidate.json')
  save(candidateFile, { source_sha: sourceSha, platform, sha256: acceptance.hash(Buffer.from('candidate')),
    models, asset: `nightly-${sourceSha}-linux-${packageName}` })
  const leasesFile = path.join(root, 'leases.json')
  save(leasesFile, [{ provider: 'openai-codex', access: 'private-token', account_id: 'account', expires_ms: Date.now() + 3_600_000 }])
  const children = new Map()
  const checkpoints = {}
  let profile, payloadChecks = 0
  const spawn = (_file, args, { env }) => {
    profile = path.dirname(env.MUNIMENT_STATE_DIR)
    const plan = read(path.join(env.MUNIMENT_STATE_DIR, 'subscription-probe.json'))
    const proof = read(path.join(output, 'release-acceptance.json'))
    checkpoints[plan.phase] = proof
    const child = Object.assign(new EventEmitter(), { pid: 100 + children.size, exitCode: null, signalCode: null })
    children.set(child.pid, child)
    const turns = plan.turns ?? models.map((_model, index) => ({ index,
      thread: '11111111-1111-4111-8111-111111111111', run: `22222222-2222-4222-8222-22222222222${index}`,
      rendered: true, context: true }))
    const features = plan.phase === 'chat' ? { 'local-startup': acceptance.featureChecks['local-startup'] }
      : plan.phase === 'features' ? { ...acceptance.featureChecks, ...featureFailures }
        : plan.phase === 'restart' ? { 'restart-persistence': acceptance.featureChecks['restart-persistence'] }
          : { 'signed-update': updateFailure ? ['failed', 'restore', 'check-failed'] : acceptance.featureChecks['signed-update'].slice(0, -1) }
    save(path.join(env.MUNIMENT_STATE_DIR, 'subscription-probe-result.json'), {
      passed: plan.phase !== failAt, source_sha: sourceSha, webdriver: false, turns, features,
      phase: plan.phase === 'update' ? 'update-restart' : plan.phase,
      pid: plan.phase === 'update' ? 200 : child.pid, update_parent_pid: child.pid,
    })
    save(plan.mcpReceipt, { token: plan.mcpNonce })
    if (plan.phase === 'chat') {
      fs.writeFileSync(path.join(env.PI_CODING_AGENT_DIR, 'subscription-probe-transports.jsonl'), models.map(({ id }) => JSON.stringify({
        purpose: 'chat', requested: id, actual: id, finished: true, subscription: true, tools: 0,
        reply_sha256: acceptance.hash(Buffer.from(plan.nonce)),
      })).join('\n'))
    }
    return child
  }
  // Exercise the runner's phase order with fake native processes and real evidence files.
  const run = vm.runInNewContext(`(${runner.run.toString()})`, {
    ...runner, ...acceptance, ...diagnostics, fs, os, path, Buffer, setTimeout, clearTimeout,
    json: read, save, spawn, delay: async () => {}, randomBytes: size => Buffer.alloc(size, 1),
    nativePlatform: () => platform,
    process: { platform, arch: 'x64', execPath: process.execPath, env: { MUNIMENT_NATIVE_DISPOSABLE_USER: '1' },
      kill: (pid, signal) => { if (signal === 'SIGKILL') children.get(-pid)?.emit('exit', 0) } },
    verifyUpdaterSignature: () => `file:${packageName}\tversion:1.0.0`,
    verifyInstalled: () => () => { payloadChecks++ },
    updateFixture: async () => ({ url: 'https://localhost/update', close: async () => {
      if (failAt === 'cleanup') throw new Error('The update server did not close.')
    } }),
    screenshot: (_platform, _pid, file) => {
      if (failAt === 'screenshot') throw screenshotError ?? new Error('The screenshot failed.')
      fs.writeFileSync(file, 'capture')
    },
    execute: (_command, args) => { fs.cpSync(args[1], args[2], { recursive: true }) },
  })
  const status = await run({ candidateFile, packageFile, executable: packageFile, leasesFile, output,
    signatureFile: packageFile, publicKeyFile: packageFile, sourceSha, platform })
  assert.equal(fs.existsSync(profile), false)
  assert.ok(payloadChecks > 0)
  const proof = read(path.join(output, 'release-acceptance.json'))
  const evidence = read(path.join(output, `${platform}-subscription.json`))
  if (evidence.status !== 'passed') {
    const collected = path.join(root, 'collected')
    const log = t.mock.method(console, 'error', () => {})
    try {
      assert.equal(collect(sourceSha, collected, [output]), 1)
      assert.deepEqual(read(path.join(collected, 'release-acceptance.json')).cases.filter(item => item.platform === platform), proof.cases)
      assert.deepEqual(read(path.join(collected, `${platform}-subscription.json`)).features, evidence.features)
    } finally { log.mock.restore() }
  }
  return { status, checkpoints, proof, evidence }
}

function assertFailure(result, feature, expected) {
  const item = result.proof.cases.find(item => item.feature === feature)
  assert.equal(item.status, 'blocked')
  assert.equal(item.failure_stage, expected[1])
  assert.equal(item.error_class, expected[2])
  assert.deepEqual(result.evidence.features[feature], expected)
}

test('the runner preserves the update restore failure after verification rejects it', async t => {
  const result = await scenario(t, { updateFailure: true })
  assert.equal(result.status, 1)
  assertFailure(result, 'signed-update', ['failed', 'restore', 'check-failed'])
  assert.deepEqual(result.proof.packages, {})
  assert.ok(result.proof.cases.every(item => item.status === 'blocked' && item.installed === false))
})

for (const failAt of ['restart', 'update', 'screenshot', 'cleanup']) {
  test(`the runner preserves feature failures after ${failAt} fails`, async t => {
    const result = await scenario(t, { featureFailures: { mcp: mcpFailure, artifacts: artifactFailure }, failAt })
    assert.equal(result.status, 1)
    assertFailure(result, 'mcp', mcpFailure)
    assertFailure(result, 'artifacts', artifactFailure)
    for (const phase of ['restart', ...(failAt === 'restart' ? [] : ['update'])]) {
      assert.equal(result.checkpoints[phase].cases.find(item => item.feature === 'mcp').failure_stage, 'connection-test')
      assert.equal(result.checkpoints[phase].cases.find(item => item.feature === 'artifacts').failure_stage, 'save')
      assert.ok(result.checkpoints[phase].cases.every(item => item.status === 'blocked'))
    }
  })
}

for (const [code, reason] of Object.entries(runner.screenshotReasons)) {
  test(`the runner and collector retain the ${code} screenshot reason`, async t => {
    const result = await scenario(t, { failAt: 'screenshot', featureFailures: { mcp: mcpFailure },
      screenshotError: new runner.ScreenshotError(code, 'The native capture failed.') })
    assert.equal(result.status, 1)
    assert.equal(result.evidence.reason, reason)
    assert.ok(result.proof.cases.every(item => item.status === 'blocked' && item.reason === reason))
    assertFailure(result, 'mcp', mcpFailure)
  })
}

test('the runner keeps the feature failure gate when later phases pass', async t => {
  const result = await scenario(t, { featureFailures: { mcp: mcpFailure } })
  assert.equal(result.status, 1)
  assertFailure(result, 'mcp', mcpFailure)
  assert.equal(result.proof.cases.find(item => item.feature === 'signed-update').status, 'passed')
})

for (const reason of ['reply-phase', 'reply-phase-failed', 'reply-phase-cancelled', 'reply-phase-interrupted',
  'reply-phase-pending-permission', 'reply-text', 'receipt-tool', ...acceptance.interruptionErrors]) {
  test(`the runner preserves the MCP ${reason} sub-reason through collection`, async t => {
    const failure = ['failed', 'tool-turn', reason]
    const result = await scenario(t, { featureFailures: { mcp: failure }, failAt: 'update' })
    assert.equal(result.status, 1)
    assertFailure(result, 'mcp', failure)
    assert.equal(result.checkpoints.restart.cases.find(item => item.feature === 'mcp').error_class, reason)
  })
}

test('the runner preserves command and assertion reasons through collection', async t => {
  for (const reasons of [
    { routing: 'model_router_save_routes', memory: 'memory_profile_save', tools: 'chat_thread_open', terminal: 'terminal_read' },
    { routing: 'fallback-selected', memory: 'profile-restored', tools: 'composer-visible', terminal: 'shell-output' },
  ]) {
    const featureFailures = Object.fromEntries(Object.entries(reasons).map(([feature, reason]) => [feature, ['failed', 'check', reason]]))
    const result = await scenario(t, { featureFailures, failAt: 'update' })
    assert.equal(result.status, 1)
    for (const [feature, failure] of Object.entries(featureFailures)) {
      assertFailure(result, feature, failure)
      assert.equal(result.checkpoints.restart.cases.find(item => item.feature === feature).error_class, failure[2])
    }
  }
})

test('The runner preserves terminal diagnostics through restart, update failure, and collection.', async t => {
  const failure = ['failed', 'check', 'shell-output', 'bytes-4096', 'received-true', 'nonce-true', 'vt-true']
  const result = await scenario(t, { featureFailures: { terminal: failure }, failAt: 'update' })
  assertFailure(result, 'terminal', failure)
  const expected = { byte_count: 4096, received: true, nonce_present: true, vt_present: true }
  assert.deepEqual(result.proof.cases.find(item => item.feature === 'terminal').terminal_diagnostics, expected)
  assert.deepEqual(result.checkpoints.restart.cases.find(item => item.feature === 'terminal').terminal_diagnostics, expected)
})

test('The terminal diagnostic grammar rejects invalid counts, flags, and extra text.', () => {
  const valid = ['failed', 'check', 'shell-output', 'bytes-0', 'received-false', 'nonce-false', 'vt-false']
  assert.deepEqual(acceptance.featureFailure('terminal', valid).terminal_diagnostics,
    { byte_count: 0, received: false, nonce_present: false, vt_present: false })
  for (const count of ['1', '9007199254740991']) {
    assert.equal(acceptance.featureFailure('terminal', [...valid.slice(0, 3), `bytes-${count}`, 'received-true', 'nonce-false', 'vt-false'])
      .terminal_diagnostics.byte_count, Number(count))
  }
  for (const [index, values] of [
    [3, ['bytes--1', 'bytes-01', 'bytes-1.5', 'bytes-1e3', 'bytes-NaN', 'bytes-Infinity', 'bytes-9007199254740992', 'bytes-0\n', 0, null, {}]],
    [4, ['received-true', 'received-false private', false]],
    [5, ['nonce-true', 'nonce-false private', false]],
    [6, ['vt-true', 'vt-false private', false]],
  ]) {
    for (const value of values) {
      const invalid = [...valid]
      invalid[index] = value
      assert.deepEqual(acceptance.featureFailure('terminal', invalid), {})
    }
  }
  for (const invalid of [valid.slice(0, -1), [...valid, 'private'], ['failed', 'restore', ...valid.slice(2)]]) {
    assert.deepEqual(acceptance.featureFailure('terminal', invalid), {})
  }
  assert.deepEqual(acceptance.featureFailure('files', valid), {})
})

test('The runner drops invalid terminal diagnostics from the evidence.', async t => {
  const result = await scenario(t, { failAt: 'update', featureFailures: {
    terminal: ['failed', 'check', 'shell-output', 'bytes-123', 'received-true', 'nonce-true', 'vt-true private'],
  } })
  assert.equal(result.evidence.features, undefined)
  assert.equal(JSON.stringify(result).includes('vt-true private'), false)
})

for (const reason of ['memory-save-unavailable', 'memory-save-busy', 'memory-save-home', 'memory-save-path',
  'memory-save-folder', 'memory-save-write', 'memory-save-size', 'memory-save-secret', 'memory-save-timeout', 'memory-save-rejected']) {
  test(`The collector preserves the ${reason} sub-code.`, async t => {
    const failure = ['failed', 'check', reason]
    const result = await scenario(t, { featureFailures: { memory: failure }, failAt: 'update' })
    assert.equal(result.status, 1)
    assertFailure(result, 'memory', failure)
    assert.equal(result.checkpoints.restart.cases.find(item => item.feature === 'memory').error_class, reason)
  })
}

test('the runner still passes complete feature evidence', async t => {
  const result = await scenario(t)
  assert.equal(result.status, 0)
  assert.ok(result.proof.cases.every(item => item.status === 'passed'))
})

test('the runner drops unknown failure stages and provider text from checkpoints', async t => {
  const result = await scenario(t, { failAt: 'update', featureFailures: {
    mcp: ['failed', 'private-provider-text', 'timeout'], artifacts: ['failed', 'save', 'private-provider-text'],
    settings: ['failed', 'check', 'check-failed', 'private-provider-text'],
    'private-provider-text': ['failed', 'check', 'check-failed'],
  } })
  assert.equal(result.status, 1)
  assert.equal(result.evidence.features, undefined)
  assert.equal(JSON.stringify(result).includes('private-provider-text'), false)
})

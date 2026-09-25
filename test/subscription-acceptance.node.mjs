import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import vm from 'node:vm'
import { acceptance, blocked, checkIdentity, hash, platforms, subscriptionAccounts } from './e2e/support/subscription-acceptance.mjs'
import { isolatedEnvironment, run, tree } from './e2e/runner/subscriptions.mjs'
import { collect } from './e2e/runner/collect-subscriptions.mjs'

const sourceSha = 'a'.repeat(40)
const bytes = Buffer.from('signed package fixture')
const models = Array.from({ length: 4 }, (_, index) => ({ family: 'openai', id: `model-${index}` }))
const candidate = { source_sha: sourceSha, sha256: hash(bytes), platform: 'linux',
  asset: `nightly-${sourceSha}-linux-muniment_1.0.0_amd64.AppImage`, models }
const nonce = 'MUNIMENT-' + 'c'.repeat(32)
const thread = '11111111-1111-4111-8111-111111111111'
function fixture() {
  const turns = models.map((model, index) => ({ index, thread,
    run: `22222222-2222-4222-8222-22222222222${index}`, rendered: true, context: true,
    requested: model.id, expected: nonce }))
  return { result: { status: 'passed', installed: true, unchanged: true, webdriver: false, source_sha: sourceSha,
    package_sha256: candidate.sha256, turns },
  transports: turns.map(turn => ({ requested: turn.requested, actual: turn.requested, subscription: true,
    finished: true, tools: 0, reply_sha256: hash(Buffer.from(nonce)) })) }
}
const build = ({ result, transports }, identity = candidate) => acceptance(identity, sourceSha, 'linux', result, transports, 'muniment_1.0.0_amd64.AppImage')
const lease = { provider: 'openai-codex', access: 'fixture-access', account_id: 'fixture-account', expires_ms: Date.now() + 30 * 60_000 }

test('the runner emits only the three subscription cases in the consumer schema', () => {
  const proof = build(fixture())
  assert.equal(proof.schema, 1)
  assert.equal(proof.source_sha, sourceSha)
  assert.deepEqual(proof.packages, { 'muniment_1.0.0_amd64.AppImage': candidate.sha256 })
  assert.deepEqual(proof.cases.map(item => item.feature), ['chat', 'direct-model-selection', 'model-switching'])
  for (const item of proof.cases) {
    assert.equal(item.status, 'passed')
    assert.equal(item.installed, true)
    assert.equal(item.evidence, 'linux-subscription.json')
    assert.equal(new Set(item.models.map(model => model.actual)).size, 4)
    for (const model of item.models) {
      assert.equal(model.requested, model.actual)
      assert.equal(model.reply, nonce)
      assert.equal(model.subscription, true)
    }
  }
})

for (const [name, mutate] of [
  ['wrong model', f => { f.transports[1].actual = 'other-model' }],
  ['missing model', f => { delete f.transports[1].actual }],
  ['API key', f => { f.transports[1].subscription = false }],
  ['failed reply', f => { f.transports[1].finished = false }],
  ['empty reply', f => { f.transports[1].reply_sha256 = hash(Buffer.from('')) }],
  ['lost context', f => { f.result.turns[2].context = false }],
  ['new thread', f => { f.result.turns[2].thread = '33333333-3333-4333-8333-333333333333' }],
  ['replayed run', f => { f.result.turns[1].run = f.result.turns[0].run }],
  ['stale result', f => { f.result.source_sha = 'b'.repeat(40) }],
  ['wrong package', f => { f.result.package_sha256 = 'b'.repeat(64) }],
  ['changed install', f => { f.result.unchanged = false }],
  ['source build', f => { f.result.installed = false }],
  ['WebDriver replacement', f => { f.result.webdriver = true }],
  ['missing compiled identity', f => { delete f.result.source_sha }],
  ['empty turns', f => { f.result.turns = [] }],
  ['extra transport attempt', f => { f.transports.push(f.transports[0]) }],
  ['unrendered reply', f => { f.result.turns[1].rendered = false }],
  ['tool use', f => { f.transports[1].tools = 1 }],
  ['out-of-order receipt', f => { f.transports.reverse() }],
  ['untrusted expected reply', f => { f.result.turns[1].expected = 'private conversation' }],
]) test(`the proof rejects ${name}`, () => {
  const f = fixture()
  mutate(f)
  assert.throws(() => build(f))
})

test('the package binds the source, platform, asset name, and digest', () => {
  assert.equal(checkIdentity(candidate, sourceSha, 'linux', bytes), 'muniment_1.0.0_amd64.AppImage')
  for (const changed of [{ source_sha: 'b'.repeat(40) }, { platform: 'windows' }, { sha256: 'b'.repeat(64) },
    { asset: `nightly-${'b'.repeat(40)}-linux-muniment.AppImage` }, { asset: candidate.asset + '/../other' }]) {
    assert.throws(() => checkIdentity({ ...candidate, ...changed }, sourceSha, 'linux', bytes))
  }
  assert.throws(() => checkIdentity(candidate, sourceSha, 'linux', Buffer.from('changed bytes')))
})

test('the lease importer keeps refresh ownership and consolidates one provider account', () => {
  const before = JSON.stringify(lease)
  const accounts = subscriptionAccounts(models, [lease])
  assert.equal(accounts.length, 1)
  assert.equal(accounts[0].credential.type, 'subscription')
  assert.deepEqual(accounts[0].models, models.map(model => model.id))
  assert.equal(JSON.stringify(lease), before)
  assert.equal('refresh' in accounts[0].credential, false)
})

for (const [name, leases] of [
  ['absent subscriptions', []], ['expired lease', [{ ...lease, expires_ms: Date.now() }]],
  ['string expiry', [{ ...lease, expires_ms: String(lease.expires_ms) }]],
  ['unbounded expiry', [{ ...lease, expires_ms: Infinity }]],
  ['refresh token', [{ ...lease, refresh: 'factory-owned' }]],
  ['gh credential', [{ ...lease, gh_token: 'factory-owned' }]],
  ['duplicate lease', [lease, lease]], ['missing account ID', [{ ...lease, account_id: '' }]],
  ['header injection', [{ ...lease, access: 'value\nother-header' }]],
  ['unsupported subscription', [{ ...lease, provider: 'google' }]],
]) test(`the importer blocks ${name}`, () => assert.throws(() => subscriptionAccounts(models, leases)))

test('the lease expiry boundary requires 20 full minutes', () => {
  const now = 1000
  assert.equal(subscriptionAccounts(models, [{ ...lease, expires_ms: now + 20 * 60_000 }], now).length, 1)
  assert.throws(() => subscriptionAccounts(models, [{ ...lease, expires_ms: now + 20 * 60_000 - 1 }], now))
})

test('the importer rejects fewer than four distinct models and local providers', () => {
  assert.throws(() => subscriptionAccounts(models.slice(1), [lease]))
  assert.throws(() => subscriptionAccounts([models[0], models[0], models[2], models[3]], [lease]))
  assert.throws(() => subscriptionAccounts(models.map(model => ({ ...model, family: 'ollama' })), [lease]))
  assert.throws(() => subscriptionAccounts(models.map(model => ({ ...model, id: '../model' })), [lease]))
  assert.throws(() => subscriptionAccounts(models.map(model => ({ ...model, id: undefined })), [lease]))
  assert.throws(() => subscriptionAccounts(models.map(model => ({ ...model, id: 1 })), [lease]))
})

test('the disposable environment does not inherit factory authority or provider homes', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-test-'))
  try {
    const env = isolatedEnvironment(root, { PATH: '/bin', GH_TOKEN: 'secret', GITHUB_TOKEN: 'secret',
      CODEX_HOME: '/shared', PI_CODING_AGENT_DIR: '/shared', HOME: '/shared', CLAUDE_CODE_OAUTH_TOKEN: 'secret',
      HTTPS_PROXY: 'https://untrusted', NODE_OPTIONS: '--require=/shared/module' })
    assert.equal(env.PATH, '/bin')
    for (const name of ['GH_TOKEN', 'GITHUB_TOKEN', 'CODEX_HOME', 'CLAUDE_CODE_OAUTH_TOKEN', 'HTTPS_PROXY', 'NODE_OPTIONS']) assert.equal(env[name], undefined)
    assert.equal(env.MUNIMENT_STATE_DIR, path.join(root, 'state'))
    assert.equal(env.PI_CODING_AGENT_DIR, path.join(root, 'state', 'agent'))
    assert.equal(env.HOME, path.join(root, 'home'))
    assert.deepEqual(tree(env.MUNIMENT_STATE_DIR), {})
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('the collector generates twelve cases and blocks missing or contradictory platform evidence', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-collect-'))
  const output = path.join(root, 'out')
  const inputs = []
  try {
    for (const platform of platforms) {
      const directory = path.join(root, platform)
      inputs.push(directory)
      fs.mkdirSync(directory)
      const f = fixture()
      const proof = acceptance({ ...candidate, platform }, sourceSha, platform, f.result, f.transports, `${platform}.package`)
      fs.writeFileSync(path.join(directory, 'release-acceptance.json'), JSON.stringify(proof))
      fs.writeFileSync(path.join(directory, `${platform}-subscription.json`), JSON.stringify({ ...f.result, transports: f.transports }))
      fs.writeFileSync(path.join(directory, `screenshot-${platform}-subscriptions.png`),
        Buffer.from(fs.readFileSync('test/e2e/fixtures/image-token.png.base64', 'utf8'), 'base64'))
    }
    assert.throws(() => collect(sourceSha, inputs[0], inputs))
    assert.equal(collect(sourceSha, output, inputs), 0)
    const combined = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    assert.equal(combined.cases.length, 12)
    assert.equal(Object.keys(combined.packages).length, 4)
    assert.ok(combined.cases.every(item => ['chat', 'direct-model-selection', 'model-switching'].includes(item.feature)))
    assert.equal(collect(sourceSha, output, inputs.slice(1)), 1)
    assert.ok(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases
      .filter(item => item.platform === 'linux').every(item => item.status === 'blocked'))
    assert.equal(collect('b'.repeat(40), output, inputs), 1)
    assert.equal(collect(sourceSha, output, [...inputs, inputs[0]]), 1)
    fs.rmSync(path.join(inputs[0], 'screenshot-linux-subscriptions.png'))
    assert.equal(collect(sourceSha, output, inputs), 1)
    assert.equal(fs.existsSync(path.join(output, 'screenshot-linux-subscriptions.png')), false)
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

for (const mode of ['success', 'wrong-selection', 'failed-reply', 'lost-context', 'new-thread']) {
  test(`the installed webview probe checks ${mode}`, async () => {
    let selected = 0, picker = false, observed, clock = 0
    const prompts = [], entries = []
    const composer = { value: '', dispatchEvent() {}, getClientRects: () => [1] }
    const responses = () => entries.map(entry => ({ getClientRects: () => [1], querySelector: selector =>
      selector === '.response-prose' ? { textContent: entry.text } : {} }))
    const send = { textContent: 'Send', getAttribute: () => 'Send', getClientRects: () => [1], click: () => {
      prompts.push(composer.value)
      entries.push({ runId: `22222222-2222-4222-8222-22222222222${entries.length}`,
        phase: mode === 'failed-reply' ? 'failed' : 'complete', text: mode === 'lost-context' ? 'unknown' : nonce })
    } }
    const document = {
      head: { append() {} }, createElement: () => ({}),
      querySelector: selector => selector.startsWith('textarea') ? composer : selector === '.model-chip'
        ? { click: () => { picker = true } } : picker ? {} : null,
      querySelectorAll: selector => selector === '.response' ? responses() : selector === 'button' ? [send]
        : models.map((model, index) => ({ dataset: { provider: 'muniment-router', model: `${model.family}/${model.id}` },
          click: () => { selected = index; picker = false } })),
    }
    const invoke = async (command, payload) => {
      if (command === 'attach_listener_status') return { supervisor_running: true, connected: true }
      if (command === 'local_mode_provider_inventory') return { default_provider: 'muniment-router',
        default_model: `openai/${mode === 'wrong-selection' ? 'wrong' : models[selected].id}` }
      if (command === 'chat_current_thread') return mode === 'new-thread' && entries.length > 1
        ? '33333333-3333-4333-8333-333333333333' : thread
      if (command === 'chat_thread_open') return { entries }
      if (command === 'subscription_probe_observed') { observed = payload; return }
      throw new Error('Unexpected probe command.')
    }
    await vm.runInNewContext(fs.readFileSync('test/e2e/support/subscription-probe.js', 'utf8'), {
      window: { __MUNIMENT_SUBSCRIPTION_PLAN__: { models, nonce }, __TAURI__: { core: { invoke } } },
      document, Event: class {}, setTimeout: callback => callback(), requestAnimationFrame: callback => callback(),
      Date: { now: () => { clock += 1000; return clock } },
    })
    assert.equal(observed.passed, mode === 'success')
    if (mode === 'success') {
      assert.equal(observed.turns.length, 4)
      assert.ok(prompts[0].includes(nonce))
      assert.ok(prompts.slice(1).every(prompt => !prompt.includes(nonce)))
      assert.equal(new Set(observed.turns.map(turn => turn.thread)).size, 1)
      assert.ok(!JSON.stringify(observed).includes(nonce))
    }
  })
}

test('each absent native runner emits actionable blocked cases and replaces stale evidence', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-blocked-'))
  try {
    await assert.rejects(run({ output: root, sourceSha, platform: '../outside' }))
    await assert.rejects(run({ output: root, sourceSha: 'wrong', platform: 'linux' }))
    assert.equal(fs.readdirSync(root).length, 0)
    for (const platform of platforms) {
      fs.writeFileSync(path.join(root, 'release-acceptance.json'), JSON.stringify(build(fixture())))
      assert.equal(await run({ output: root, sourceSha, platform }), 1)
      const proof = JSON.parse(fs.readFileSync(path.join(root, 'release-acceptance.json')))
      assert.equal(proof.cases.length, 3)
      assert.ok(proof.cases.every(item => item.status === 'blocked' && item.installed === false && item.reason.length > 20))
      assert.deepEqual(proof.packages, {})
      assert.ok(fs.statSync(path.join(root, `${platform}-subscription.json`)).size > 0)
    }
    assert.equal(blocked(sourceSha, 'linux', 'Provide subscriptions.').cases[0].status, 'blocked')
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

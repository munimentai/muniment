import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import vm from 'node:vm'
import { generateKeyPairSync, randomBytes } from 'node:crypto'
import { acceptance, blocked, checkIdentity, hash, platforms, subscriptionAccounts } from './e2e/support/subscription-acceptance.mjs'
import { isolatedEnvironment, run, tree, updaterPublicKeyFile, writeBlocked } from './e2e/runner/subscriptions.mjs'
import { collect } from './e2e/runner/collect-subscriptions.mjs'
import { signUpdaterBytes, decodePublicKey } from '../.github/lib/updater-signature.mjs'
import { host } from './e2e/runner/subscription-host.mjs'
import { guest, selectAssets, defaultArtifactsDir } from './e2e/runner/subscription-guest.mjs'

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

test('the collector preserves blocked runner reasons and still fails', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-collect-blocked-'))
  const output = path.join(root, 'out')
  const inputs = []
  const reason = 'Provide the FACTORY_SUBSCRIPTION_LEASES secret with access-only factory leases.'
  try {
    for (const platform of platforms) {
      const directory = path.join(root, platform)
      inputs.push(directory)
      fs.mkdirSync(directory)
      const proof = blocked(sourceSha, platform, reason)
      fs.writeFileSync(path.join(directory, 'release-acceptance.json'), JSON.stringify(proof))
      fs.writeFileSync(path.join(directory, `${platform}-subscription.json`), JSON.stringify({ status: 'blocked', reason }))
    }
    assert.equal(collect(sourceSha, output, inputs), 1)
    const combined = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    assert.equal(combined.cases.length, 12)
    assert.ok(combined.cases.every(item => item.status === 'blocked' && item.reason === reason))
    assert.deepEqual(combined.packages, {})
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

function testKey() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519')
  const pk = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)
  const keyId = randomBytes(8)
  const publicText = `untrusted comment: minisign public key: ${keyId.toString('hex').toUpperCase()}\n${Buffer.concat([Buffer.from('Ed', 'latin1'), keyId, pk]).toString('base64')}\n`
  return { key: { keyId, privateKey }, publicText }
}

const nativeLinux = process.platform === 'linux' && process.arch === 'x64'
const identityReason = 'The candidate identity, package digest, or updater signature is invalid.'
const payloadReason = 'The installed payload does not match the signed package. Check native package and signature tools.'

async function identityRun(mutate, publicKeyFile = true) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-identity-'))
  const previous = process.env.MUNIMENT_NATIVE_DISPOSABLE_USER
  process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = '1'
  try {
    const keys = testKey()
    const keyFile = path.join(root, 'updater.pub')
    fs.writeFileSync(keyFile, keys.publicText)
    const packageFile = path.join(root, 'package')
    fs.writeFileSync(packageFile, bytes)
    const packageName = 'muniment_1.0.0_amd64.AppImage'
    const signatureFile = path.join(root, 'package.sig')
    fs.writeFileSync(signatureFile, signUpdaterBytes(bytes, keys.key, { fileName: packageName, version: '1.0.0' }))
    const candidateFile = path.join(root, 'candidate.json')
    fs.writeFileSync(candidateFile, JSON.stringify(candidate))
    const executable = path.join(root, 'app')
    fs.writeFileSync(executable, Buffer.from('different installed bytes'))
    const leasesFile = path.join(root, 'leases.json')
    fs.writeFileSync(leasesFile, JSON.stringify([lease]), { mode: 0o600 })
    const output = path.join(root, 'out')
    mutate?.({ packageFile, signatureFile, candidateFile, keyFile, keys, packageName })
    const code = await run({
      candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform: 'linux',
      ...(publicKeyFile === true ? { publicKeyFile: keyFile } : publicKeyFile === null ? {} : { publicKeyFile }),
    })
    const proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    return { code, proof, reason: proof.cases[0].reason }
  } finally {
    if (previous === undefined) delete process.env.MUNIMENT_NATIVE_DISPOSABLE_USER
    else process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = previous
    fs.rmSync(root, { recursive: true, force: true })
  }
}

test('the runner reads the committed updater public key', () => {
  assert.equal(updaterPublicKeyFile, 'src-tauri/updater.pub')
  decodePublicKey(fs.readFileSync(updaterPublicKeyFile, 'utf8'))
  const source = fs.readFileSync('test/e2e/runner/subscriptions.mjs', 'utf8')
  assert.equal(source.includes("json('src-tauri/tauri.conf.json').plugins.updater.pubkey"), false)
  assert.match(source, /src-tauri\/updater\.pub/)
})

test('a valid updater signature passes the identity step', { skip: !nativeLinux }, async () => {
  const result = await identityRun()
  assert.equal(result.code, 1)
  assert.equal(result.reason, payloadReason)
  assert.ok(result.proof.cases.every(item => item.status === 'blocked'))
})

test('a tampered package produces blocked identity evidence', { skip: !nativeLinux }, async () => {
  const result = await identityRun(files => {
    const tampered = Buffer.from('tampered package fixture')
    fs.writeFileSync(files.packageFile, tampered)
    fs.writeFileSync(files.candidateFile, JSON.stringify({ ...candidate, sha256: hash(tampered) }))
  })
  assert.equal(result.code, 1)
  assert.equal(result.reason, identityReason)
})

test('a wrong file comment produces blocked identity evidence', { skip: !nativeLinux }, async () => {
  const result = await identityRun(files => {
    fs.writeFileSync(files.signatureFile, signUpdaterBytes(bytes, files.keys.key, { fileName: 'other.AppImage', version: '1.0.0' }))
  })
  assert.equal(result.code, 1)
  assert.equal(result.reason, identityReason)
})

test('a wrong updater public key produces blocked identity evidence', { skip: !nativeLinux }, async () => {
  const other = path.join(os.tmpdir(), `subscription-wrong-key-${randomBytes(8).toString('hex')}.pub`)
  try {
    fs.writeFileSync(other, testKey().publicText)
    const keyed = await identityRun(undefined, other)
    assert.equal(keyed.code, 1)
    assert.equal(keyed.reason, identityReason)
    const fallback = await identityRun(undefined, null)
    assert.equal(fallback.code, 1)
    assert.equal(fallback.reason, identityReason)
  } finally { fs.rmSync(other, { force: true }) }
})

test('the host publishes blocked cases when leases, models, or native runners are missing', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-host-'))
  try {
    const output = path.join(root, 'out')
    let called = 0
    const invoke = () => { called += 1; return { status: 0 } }
    const modelsJson = JSON.stringify(models)
    const leasesJson = JSON.stringify([lease])
    assert.equal(host({ sourceSha, platform: 'linux', output, leases: '', models: modelsJson, sshKey: 'k', knownHosts: 'h', invoke }), 1)
    assert.match(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases[0].reason, /FACTORY_SUBSCRIPTION_LEASES/)
    assert.equal(host({ sourceSha, platform: 'windows', output, leases: leasesJson, models: '', sshKey: 'k', knownHosts: 'h', invoke }), 1)
    assert.match(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases[0].reason, /FACTORY_SUBSCRIPTION_MODELS/)
    assert.equal(host({ sourceSha, platform: 'macos-arm64', output, leases: '{', models: modelsJson, sshKey: 'k', knownHosts: 'h', invoke }), 1)
    assert.match(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases[0].reason, /FACTORY_SUBSCRIPTION_LEASES/)
    assert.equal(host({ sourceSha, platform: 'macos-x64', output, leases: leasesJson, models: modelsJson, sshKey: '', knownHosts: 'h', invoke }), 1)
    assert.match(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases[0].reason, /native desktop-ci runner/)
    assert.equal(called, 0)
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('the host copies guest evidence and keeps a failed native check red', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-host-copy-'))
  try {
    const output = path.join(root, 'out')
    const code = host({
      sourceSha, platform: 'linux', output, leases: JSON.stringify([lease]), models: JSON.stringify(models),
      sshKey: 'k', knownHosts: 'h', repository: 'owner/repo', token: 't',
      invoke: ({ output: artifacts, platform, subscriptionPlatform }) => {
        assert.equal(platform, 'linux')
        assert.equal(subscriptionPlatform, 'linux')
        writeBlocked(artifacts, sourceSha, 'linux', 'The installed subscription reply or model switch failed.')
        return { status: 1 }
      },
    })
    assert.equal(code, 1)
    const proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    assert.ok(proof.cases.every(item => item.status === 'blocked'))
    assert.match(proof.cases[0].reason, /subscription reply/)
    const mapped = path.join(root, 'arm64')
    assert.equal(host({
      sourceSha, platform: 'macos-arm64', output: mapped, leases: JSON.stringify([lease]), models: JSON.stringify(models),
      sshKey: 'k', knownHosts: 'h', repository: 'owner/repo', token: 't',
      invoke: ({ platform, subscriptionPlatform, output: artifacts }) => {
        assert.equal(platform, 'macos')
        assert.equal(subscriptionPlatform, 'macos-arm64')
        writeBlocked(artifacts, sourceSha, 'macos-arm64', 'The native desktop-ci runner for this platform is unavailable.')
        return { status: 1 }
      },
    }), 1)
    assert.equal(JSON.parse(fs.readFileSync(path.join(mapped, 'release-acceptance.json'))).cases[0].platform, 'macos-arm64')
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('the guest selects one signed package and signature per platform', () => {
  const release = { assets: platforms.flatMap(platform => {
    const name = platform === 'linux' ? `nightly-${sourceSha}-linux-muniment_1.0.0_amd64.AppImage`
      : platform === 'windows' ? `nightly-${sourceSha}-windows-muniment_1.0.0_x64_en-US.msi`
        : `nightly-${sourceSha}-macos-muniment-${platform.slice('macos-'.length)}.app.tar.gz`
    return [{ name, id: 10 + platforms.indexOf(platform) }, { name: `${name}.sig`, id: 20 + platforms.indexOf(platform) }]
  }) }
  for (const platform of platforms) {
    const selected = selectAssets(release, sourceSha, platform)
    assert.equal(selected.signatureAsset.name, `${selected.packageAsset.name}.sig`)
    assert.ok(selected.packageAsset.name.includes(platform === 'linux' ? 'linux' : platform === 'windows' ? 'windows' : platform.slice('macos-'.length)))
  }
  assert.throws(() => selectAssets({ assets: release.assets.filter(asset => !asset.name.endsWith('.sig')) }, sourceSha, 'linux'))
  assert.throws(() => selectAssets({ assets: [] }, sourceSha, 'windows'))
})

test('the guest publishes blocked cases when models are missing', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-guest-'))
  try {
    assert.equal(await guest({ sourceSha, platform: 'linux', output: root, leases: '[]', models: '', repository: 'owner/repo', token: 't' }), 1)
    assert.match(JSON.parse(fs.readFileSync(path.join(root, 'release-acceptance.json'))).cases[0].reason, /FACTORY_SUBSCRIPTION_MODELS/)
    assert.equal(await guest({ sourceSha, platform: 'windows', output: root, leases: '', models: '[]', repository: 'owner/repo', token: 't' }), 1)
    assert.match(JSON.parse(fs.readFileSync(path.join(root, 'release-acceptance.json'))).cases[0].reason, /FACTORY_SUBSCRIPTION_LEASES/)
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('the guest defaults artifacts to the desktop-ci collection directory', async () => {
  assert.equal(defaultArtifactsDir({ TEMP: path.join('C:', 'Temp') }, 'win32'), path.join('C:', 'Temp', 'dci-artifacts'))
  assert.equal(defaultArtifactsDir({ TMP: path.join('C:', 'Temp') }, 'win32'), path.join('C:', 'Temp', 'dci-artifacts'))
  assert.equal(defaultArtifactsDir({ TMPDIR: '/other', OUTPUT: '/out' }, 'linux'), '/tmp/dci-artifacts')
  assert.equal(defaultArtifactsDir({ TMPDIR: '/other' }, 'darwin'), '/tmp/dci-artifacts')
  assert.equal(defaultArtifactsDir({ DCI_ARTIFACTS_DIR: '/custom', TEMP: path.join('C:', 'Temp'), OUTPUT: '/out' }, 'win32'), '/custom')
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-guest-default-'))
  try {
    const temp = path.join(root, 'Temp')
    fs.mkdirSync(temp)
    assert.equal(await guest({
      sourceSha, platform: 'windows', leases: '', models: '[]', repository: 'owner/repo', token: 'secret-token',
      env: { TEMP: temp }, runtime: 'win32',
    }), 1)
    const output = path.join(temp, 'dci-artifacts')
    const proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'), 'utf8'))
    assert.match(proof.cases[0].reason, /FACTORY_SUBSCRIPTION_LEASES/)
    assert.equal(JSON.stringify(proof).includes('secret-token'), false)
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('the guest loads the nightly package through the GitHub REST API', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-guest-fetch-'))
  const token = 'secret-token-value'
  const repository = 'munimentai/muniment'
  const packageName = `nightly-${sourceSha}-linux-muniment_1.0.0_amd64.AppImage`
  const release = { assets: [{ name: packageName, id: 101 }, { name: `${packageName}.sig`, id: 202 }] }
  const calls = []
  const previousFetch = globalThis.fetch
  const previousError = console.error
  const previousUser = process.env.MUNIMENT_NATIVE_DISPOSABLE_USER
  const logs = []
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), headers: init?.headers ?? {} })
    const href = String(url)
    if (href === `https://api.github.com/repos/${repository}/releases/tags/nightly`) {
      return { ok: true, json: async () => release }
    }
    if (href === `https://api.github.com/repos/${repository}/releases/assets/101`) {
      return { ok: true, arrayBuffer: async () => Buffer.from('package-bytes') }
    }
    if (href === `https://api.github.com/repos/${repository}/releases/assets/202`) {
      return { ok: true, arrayBuffer: async () => Buffer.from('signature-bytes') }
    }
    throw new Error(`unexpected ${href}`)
  }
  console.error = message => { logs.push(String(message)) }
  try {
    assert.equal(await guest({
      sourceSha, platform: 'linux', output: root, leases: JSON.stringify([lease]),
      models: JSON.stringify(models), repository, token,
    }), 1)
    assert.equal(calls.length, 3)
    assert.equal(calls[0].url, `https://api.github.com/repos/${repository}/releases/tags/nightly`)
    assert.equal(calls[0].headers.Authorization, `Bearer ${token}`)
    for (const call of calls.slice(1)) {
      assert.match(call.url, /^https:\/\/api\.github\.com\/repos\/munimentai\/muniment\/releases\/assets\/(101|202)$/)
      assert.equal(call.headers.Authorization, `Bearer ${token}`)
      assert.equal(call.headers.Accept, 'application/octet-stream')
    }
    const proof = fs.readFileSync(path.join(root, 'release-acceptance.json'), 'utf8')
    assert.equal(proof.includes(token), false)
    assert.equal(logs.join('\n').includes(token), false)
    const source = fs.readFileSync('test/e2e/runner/subscription-guest.mjs', 'utf8')
    assert.equal(source.includes("spawnSync('gh'"), false)
    assert.match(source, /api\.github\.com/)
    calls.length = 0
    globalThis.fetch = async (_url, init) => {
      throw new Error(`401 ${init.headers.Authorization}`)
    }
    assert.equal(await guest({
      sourceSha, platform: 'linux', output: root, leases: JSON.stringify([lease]),
      models: JSON.stringify(models), repository, token,
    }), 1)
    const blockedProof = JSON.parse(fs.readFileSync(path.join(root, 'release-acceptance.json'), 'utf8'))
    assert.match(blockedProof.cases[0].reason, /signed nightly package/)
    assert.equal(JSON.stringify(blockedProof).includes(token), false)
    assert.equal(logs.join('\n').includes(token), false)
    globalThis.fetch = async () => ({ ok: false, json: async () => ({ message: token }) })
    assert.equal(await guest({
      sourceSha, platform: 'linux', output: root, leases: JSON.stringify([lease]),
      models: JSON.stringify(models), repository, token,
    }), 1)
    const failedProof = JSON.parse(fs.readFileSync(path.join(root, 'release-acceptance.json'), 'utf8'))
    assert.match(failedProof.cases[0].reason, /signed nightly package/)
    assert.equal(JSON.stringify(failedProof).includes(token), false)
  } finally {
    globalThis.fetch = previousFetch
    console.error = previousError
    if (previousUser === undefined) delete process.env.MUNIMENT_NATIVE_DISPOSABLE_USER
    else process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = previousUser
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('the workflow runs a native job per platform and uploads release-acceptance', () => {
  const workflow = fs.readFileSync('.github/workflows/subscriptions.yml', 'utf8')
  const nightly = fs.readFileSync('.github/workflows/nightly.yml', 'utf8')
  assert.match(workflow, /workflow_dispatch:/)
  assert.match(workflow, /workflow_call:/)
  assert.match(workflow, /node test\/e2e\/runner\/subscription-host\.mjs/)
  assert.match(workflow, /node test\/e2e\/runner\/collect-subscriptions\.mjs/)
  assert.match(workflow, /FACTORY_SUBSCRIPTION_LEASES: \$\{\{ secrets\.FACTORY_SUBSCRIPTION_LEASES \}\}/)
  assert.match(workflow, /FACTORY_SUBSCRIPTION_MODELS: \$\{\{ vars\.FACTORY_SUBSCRIPTION_MODELS \}\}/)
  for (const platform of platforms) {
    assert.match(workflow, new RegExp(`  ${platform}:`))
    assert.match(workflow, new RegExp(`PLATFORM: ${platform}`))
    assert.match(workflow, new RegExp(`name: subscription-${platform}`))
  }
  assert.match(workflow, /name: release-acceptance/)
  assert.match(workflow, /test "\$COLLECT_STATUS" = 0/)
  assert.match(workflow, /chat, direct-model-selection, and model-switching only/)
  assert.ok((workflow.match(/if: always\(\)/g) ?? []).length >= 6)
  assert.match(workflow, /needs: \[linux, windows, macos-arm64, macos-x64\]/)
  assert.equal(nightly.includes('subscription-host.mjs'), false)
  assert.equal(nightly.includes('FACTORY_SUBSCRIPTION_LEASES'), false)
  assert.equal(workflow.includes('echo $FACTORY_SUBSCRIPTION_LEASES'), false)
  const hostSource = fs.readFileSync('test/e2e/runner/subscription-host.mjs', 'utf8')
  assert.equal(hostSource.includes('console.log(leases'), false)
  assert.equal(hostSource.includes('console.error(leases'), false)
})

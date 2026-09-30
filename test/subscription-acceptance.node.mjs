import './subscription-features.node.mjs'
import './subscription-diagnostics.node.mjs'
import './subscription-linux-sandbox.node.mjs'
import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import vm from 'node:vm'
import { spawnSync } from 'node:child_process'
import { generateKeyPairSync, randomBytes } from 'node:crypto'
import { AcceptanceError, acceptance, blocked, chatTransports, checkIdentity, hash, platforms, subscriptionAccounts, features, featureChecks, chatFeatures } from './e2e/support/subscription-acceptance.mjs'
import { assertAttachSocketPath, disposableProfilePrefix, isolatedEnvironment, run, tree, updaterPublicKeyFile, writeBlocked } from './e2e/runner/subscriptions.mjs'
import { collect } from './e2e/runner/collect-subscriptions.mjs'
import { signUpdaterBytes, decodePublicKey } from '../.github/lib/updater-signature.mjs'
import { host, runDesktopCi } from './e2e/runner/subscription-host.mjs'
import { guest, guestFailureReason, runMacosProbe, selectAssets, defaultArtifactsDir, decodeSubscriptionPayload } from './e2e/runner/subscription-guest.mjs'
import { sandboxRequirement } from './e2e/runner/subscription-linux-sandbox.mjs'
import { hostedArm64 } from './e2e/runner/subscription-macos-arm64.mjs'
import { upload } from '../.github/lib/artifact-store.mjs'

test('subscription PowerShell file launches bypass the execution policy', () => {
  const files = ['test/subscription-windows-process.node.mjs']
  for (const directory of ['test/e2e/runner', 'test/e2e/support']) {
    for (const name of fs.readdirSync(directory)) {
      if (name.startsWith('subscription')) files.push(path.join(directory, name))
    }
  }
  let launches = 0
  for (const file of files) {
    const source = fs.readFileSync(file, 'utf8')
    const commands = [...source.matchAll(/['"]powershell\.exe['"]\s*,\s*\[([^\]]*)\]/gi)]
      .map(match => [...match[1].matchAll(/['"]([^'"]*)['"]/g)].map(argument => argument[1]))
    // Generated PowerShell fixtures use ProcessStartInfo rather than Node spawn.
    for (const match of source.matchAll(/\$\w+\.Arguments\s*=\s*(['"])(.*?)\1/g)) {
      commands.push(match[2].trim().split(/\s+/))
    }
    for (const command of commands) {
      const args = command.map(argument => argument.toLowerCase())
      const fileIndex = args.indexOf('-file')
      if (fileIndex < 0) continue
      launches++
      const options = args.slice(0, fileIndex)
      const policyIndex = options.indexOf('-executionpolicy')
      assert.ok(policyIndex >= 0, `${file} must set the execution policy before -File.`)
      assert.equal(options[policyIndex + 1], 'bypass', `${file} must use ExecutionPolicy Bypass.`)
      assert.equal(options.lastIndexOf('-executionpolicy'), policyIndex, `${file} must set the execution policy once.`)
    }
  }
  assert.ok(launches >= 7, 'The scan must cover the runner, job launcher, and generated fixtures.')
})

const sourceSha = 'a'.repeat(40)
const packageNames = { linux: 'muniment_1.0.0_amd64.AppImage', windows: 'muniment_1.0.0_x64_en-US.msi',
  'macos-arm64': 'muniment-arm64.app.tar.gz', 'macos-x64': 'muniment-x64.app.tar.gz' }
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
    package_sha256: candidate.sha256, turns, features: structuredClone(featureChecks) },
  transports: turns.map(turn => ({ purpose: 'chat', requested: turn.requested, actual: turn.requested, subscription: true,
    finished: true, tools: 0, reply_sha256: hash(Buffer.from(nonce)) })) }
}
const build = ({ result, transports }, identity = candidate) => acceptance(identity, sourceSha, 'linux', result, transports, 'muniment_1.0.0_amd64.AppImage')
const lease = { provider: 'openai-codex', access: 'fixture-access', account_id: 'fixture-account', expires_ms: Date.now() + 30 * 60_000 }

test('the runner emits every required case in the consumer schema', () => {
  const proof = build(fixture())
  assert.equal(proof.schema, 1)
  assert.equal(proof.source_sha, sourceSha)
  assert.deepEqual(proof.packages, { 'muniment_1.0.0_amd64.AppImage': candidate.sha256 })
  assert.deepEqual(proof.cases.map(item => item.feature), ['chat', 'direct-model-selection', 'model-switching',
    'routing', 'account-balancing', 'settings', 'tools', 'mcp', 'files', 'restart-persistence', 'signed-update',
    'local-startup', 'projects', 'memory', 'terminal', 'agents', 'artifacts', 'browser'])
  for (const item of proof.cases) {
    assert.equal(item.status, 'passed')
    assert.equal(item.installed, true)
    assert.equal(item.evidence, 'linux-subscription.json')
    assert.equal(new Set(item.models.map(model => model.actual)).size, chatFeatures.includes(item.feature) ? 4 : 0)
    if (featureChecks[item.feature]) assert.deepEqual(item.checks, featureChecks[item.feature])
    for (const model of item.models) {
      assert.equal(model.requested, model.actual)
      assert.equal(model.reply, nonce)
      assert.equal(model.subscription, true)
    }
  }
})

for (const feature of Object.keys(featureChecks)) {
  test(`the proof requires independent runner checks for ${feature}`, () => {
    for (const invalid of [undefined, [], true, ['passed'], [...featureChecks[feature], 'extra'],
      featureChecks[feature].slice(1), [...featureChecks[feature]].reverse()]) {
      if (JSON.stringify(invalid) === JSON.stringify(featureChecks[feature])) continue
      const f = fixture()
      f.result.features[feature] = invalid
      const proof = build(f)
      assert.equal(proof.cases.find(item => item.feature === feature).status, 'blocked')
      assert.ok(proof.cases.filter(item => item.feature !== feature).every(item => item.status === 'passed'))
    }
  })
}

test('the lease importer accepts distinct accounts for balancing but rejects duplicate identities', () => {
  const second = { ...lease, access: 'second-access', account_id: 'second-account' }
  const accounts = subscriptionAccounts(models, [lease, second])
  assert.equal(accounts.length, 2)
  assert.notEqual(accounts[0].id, accounts[1].id)
  assert.equal(accounts[0].family, accounts[1].family)
  assert.throws(() => subscriptionAccounts(models, [lease, { ...second, account_id: lease.account_id }]))
  assert.throws(() => subscriptionAccounts(models, [lease, { ...second, refresh: 'private' }]))
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

for (const title of ['Token memory', nonce]) {
  test(`the chat receipts use request purpose when the title ${title === nonce ? 'equals the nonce' : 'differs from the nonce'}`, () => {
    const f = fixture()
    const naming = { ...f.transports[0], purpose: 'thread-name', reply_sha256: hash(Buffer.from(title)) }
    for (let position = 0; position <= 4; position++) {
      const receipts = [...f.transports]
      receipts.splice(position, 0, naming)
      assert.throws(() => build({ ...f, transports: receipts }), { condition: 'chat-transport-count' })
      const selected = chatTransports(receipts, models[0].id)
      assert.deepEqual(selected, f.transports)
      assert.equal(selected.length, 4)
      assert.equal(receipts.length, 5)
      assert.ok(build({ ...f, transports: selected }).cases.every(item => item.status === 'passed'))
    }
    assert.deepEqual(chatTransports(f.transports, models[0].id), f.transports)
  })
}

test('the chat receipts reject unknown purposes, extra chat receipts, and invalid naming receipts', () => {
  const f = fixture()
  const naming = { ...f.transports[0], purpose: 'thread-name' }
  for (const invalid of [null, {}, { ...naming, purpose: undefined }, { ...naming, purpose: 'PRIVATE PURPOSE' },
    { ...naming, requested: models[1].id }, { ...naming, actual: 'wrong' },
    { ...naming, subscription: false }, { ...naming, finished: false }, { ...naming, tools: 1 },
    { ...naming, reply_sha256: 'PRIVATE REPLY' }]) {
    assert.throws(() => chatTransports([invalid, ...f.transports], models[0].id),
      { condition: 'chat-transport-scope' })
  }
  for (const extra of [f.transports[0], { ...f.transports[0], reply_sha256: hash(Buffer.from('Token memory')) }]) {
    for (let position = 0; position <= 4; position++) {
      const receipts = [...f.transports]
      receipts.splice(position, 0, extra)
      for (const rows of [receipts, [naming, ...receipts]]) {
        assert.throws(() => build({ ...f, transports: chatTransports(rows, models[0].id) }),
          { condition: 'chat-transport-count' })
      }
    }
  }
  for (const invalid of [undefined, null, [], f.transports.slice(1), [naming, ...f.transports, naming],
    [naming, ...f.transports.slice(1)], [naming, ...f.transports.slice().reverse()]]) {
    assert.throws(() => build({ ...f, transports: chatTransports(invalid, models[0].id) }))
  }
  for (const mutate of [f => { f.transports[0].actual = 'wrong' }, f => { f.transports[0].finished = false },
    f => { f.transports[0].reply_sha256 = hash(Buffer.from('wrong reply')) }, f => { f.transports[0].tools = 1 }]) {
    const broken = fixture()
    mutate(broken)
    assert.throws(() => build({ ...broken, transports: chatTransports([naming, ...broken.transports], models[0].id) }))
  }
})

for (const [condition, mutate] of [
  ['candidate-source', f => { f.candidate.source_sha = 'PRIVATE SOURCE' }],
  ['candidate-platform', f => { f.candidate.platform = 'PRIVATE PLATFORM' }],
  ['candidate-digest', f => { f.candidate.sha256 = 'PRIVATE DIGEST' }],
  ['probe-status', f => { f.result.status = 'PRIVATE STATUS' }],
  ['probe-installed', f => { f.result.installed = false }],
  ['probe-unchanged', f => { f.result.unchanged = false }],
  ['probe-webdriver', f => { f.result.webdriver = true }],
  ['probe-source', f => { f.result.source_sha = 'PRIVATE SOURCE' }],
  ['probe-package-digest', f => { f.result.package_sha256 = 'PRIVATE DIGEST' }],
  ['chat-turn-count', f => { f.result.turns = [] }],
  ['chat-transport-count', f => { f.transports.push(f.transports[0]) }],
  ['turn-index', f => { f.result.turns[0] = null }],
  ['turn-thread', f => { f.result.turns[0].thread = 'PRIVATE THREAD' }],
  ['turn-run', f => { f.result.turns[0].run = 'PRIVATE RUN' }],
  ['turn-rendered', f => { f.result.turns[0].rendered = false }],
  ['turn-context', f => { f.result.turns[0].context = false }],
  ['turn-model', f => { f.result.turns[0].requested = 'PRIVATE MODEL' }],
  ['transport-requested-model', f => { f.transports[0] = null }],
  ['transport-actual-model', f => { f.transports[0].actual = 'PRIVATE MODEL' }],
  ['transport-finished', f => { f.transports[0].finished = false }],
  ['transport-subscription', f => { f.transports[0].subscription = false }],
  ['transport-tools', f => { f.transports[0].tools = 1 }],
  ['turn-expected-reply', f => { f.result.turns[0].expected = 'PRIVATE REPLY' }],
  ['transport-reply-digest', f => { f.transports[0].reply_sha256 = 'PRIVATE REPLY' }],
  ['chat-thread-continuity', f => { f.result.turns[1].thread = '33333333-3333-4333-8333-333333333333' }],
  ['chat-distinct-runs', f => { f.result.turns[1].run = f.result.turns[0].run }],
  ['chat-distinct-models', f => {
    f.result.turns[1].requested = f.result.turns[0].requested
    f.transports[1] = f.transports[0]
  }],
  ['chat-reply-continuity', f => {
    f.result.turns[1].expected = 'MUNIMENT-' + 'd'.repeat(32)
    f.transports[1].reply_sha256 = hash(Buffer.from(f.result.turns[1].expected))
  }],
]) test(`the acceptance failure records only the ${condition} label`, () => {
  const f = { ...fixture(), candidate: { ...candidate } }
  mutate(f)
  assert.throws(() => build(f, f.candidate), error => {
    assert.ok(error instanceof AcceptanceError)
    assert.equal(error.condition, condition)
    assert.equal(error.message, `The subscription acceptance condition failed: ${condition}.`)
    assert.equal(JSON.stringify(error).includes('PRIVATE'), false)
    assert.equal(JSON.stringify(error).includes(nonce), false)
    return true
  })
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
  assert.throws(() => subscriptionAccounts(models.map(model => ({ ...model, refresh: 'private' })), [lease]))
})

test('the disposable environment does not inherit factory authority or provider homes', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-test-'))
  try {
    const env = isolatedEnvironment(root, { PATH: '/bin', GH_TOKEN: 'secret', GITHUB_TOKEN: 'secret',
      CODEX_HOME: '/shared', PI_CODING_AGENT_DIR: '/shared', HOME: '/shared', CLAUDE_CODE_OAUTH_TOKEN: 'secret',
      HTTPS_PROXY: 'https://untrusted', NODE_OPTIONS: '--require=/shared/module' }, 'linux')
    assert.equal(env.PATH, '/bin')
    for (const name of ['GH_TOKEN', 'GITHUB_TOKEN', 'CODEX_HOME', 'CLAUDE_CODE_OAUTH_TOKEN', 'HTTPS_PROXY', 'NODE_OPTIONS']) assert.equal(env[name], undefined)
    assert.equal(env.MUNIMENT_STATE_DIR, path.join(root, 'state'))
    assert.equal(env.PI_CODING_AGENT_DIR, path.join(root, 'state', 'agent'))
    assert.equal(env.HOME, path.join(root, 'home'))
    assert.deepEqual(tree(env.MUNIMENT_STATE_DIR), {})
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

for (const platform of ['windows', 'macos-arm64', 'macos-x64']) {
  test(`the ${platform} environment keeps native services in the disposable login`, () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-native-test-'))
    const source = { MUNIMENT_NATIVE_DISPOSABLE_USER: '1', HOME: '/Users/disposable',
      UserProfile: 'C:\\Users\\disposable', AppData: 'C:\\Users\\disposable\\AppData\\Roaming',
      LocalAppData: 'C:\\Users\\disposable\\AppData\\Local', GH_TOKEN: 'private',
      CODEX_HOME: '/factory/codex', PI_CODING_AGENT_DIR: '/factory/agent',
      NODE_OPTIONS: '--require=/factory/module', HTTPS_PROXY: 'https://private' }
    const before = { ...source }
    try {
      const env = isolatedEnvironment(root, source, platform)
      if (platform === 'windows') {
        // Shell known folders expand the registry value through USERPROFILE, not LOCALAPPDATA.
        const registryPath = '%USERPROFILE%\\AppData\\Local'.replace('%USERPROFILE%', env.USERPROFILE)
        assert.equal(registryPath, source.LocalAppData)
        assert.equal(env.LOCALAPPDATA, source.LocalAppData)
        assert.equal(env.APPDATA, source.AppData)
        assert.equal(env.HOME, path.join(root, 'home'))
      } else {
        assert.equal(env.HOME, source.HOME)
      }
      assert.equal(env.MUNIMENT_STATE_DIR, path.join(root, 'state'))
      assert.equal(env.PI_CODING_AGENT_DIR, path.join(root, 'state', 'agent'))
      for (const name of ['GH_TOKEN', 'CODEX_HOME', 'NODE_OPTIONS', 'HTTPS_PROXY', 'MUNIMENT_NATIVE_DISPOSABLE_USER']) {
        assert.equal(env[name], undefined)
      }
      assert.deepEqual(source, before)
      assert.deepEqual(tree(env.MUNIMENT_STATE_DIR), {})
      const required = platform === 'windows' ? ['UserProfile', 'AppData', 'LocalAppData'] : ['HOME']
      for (const name of required) {
        for (const value of [undefined, '', 'relative', 0]) {
          assert.throws(() => isolatedEnvironment(root, { ...source, [name]: value }, platform), /absolute .* path/)
        }
      }
      for (const marker of [undefined, '', '0']) {
        assert.throws(() => isolatedEnvironment(root, { ...source, MUNIMENT_NATIVE_DISPOSABLE_USER: marker }, platform), /disposable native login/)
      }
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  })
}

for (const platform of ['macos-arm64', 'macos-x64']) {
  test(`the ${platform} installed probe uses one disposable Keychain session`, () => {
    const probe = { candidateFile: '/candidate.json', packageFile: '/signed package.tar.gz', signatureFile: '/package.sig',
      executable: '/installed/muniment.app/Contents/MacOS/muniment-desktop', leasesFile: '/leases.json',
      output: '/evidence', sourceSha, platform }
    let calls = 0
    const execute = (command, args, options) => {
      calls++
      assert.equal(command, '/bin/bash')
      assert.deepEqual(args, [path.resolve('test/e2e/support/macos-keychain-session.sh'), process.execPath,
        path.resolve('test/e2e/runner/subscriptions.mjs'), ...Object.values(probe)])
      assert.equal(options.encoding, 'utf8')
      assert.equal(options.stdio, 'pipe')
      assert.ok(options.timeout > 0)
      assert.equal(options.env, undefined)
      return { status: 0 }
    }
    assert.equal(runMacosProbe(probe, execute), 0)
    assert.equal(calls, 1)
    for (const result of [{ status: 78, stderr: 'Keychain access failed.' },
      { status: 1, stderr: 'Keychain cleanup failed.' }, { status: null, signal: 'SIGTERM' },
      { status: 0, error: new Error('spawn failed') }]) {
      assert.throws(() => runMacosProbe(probe, () => result), /macos-keychain-session/)
    }
  })

  test(`the ${platform} profile fits the attach socket limit despite a long GUI TMPDIR`, () => {
    const guiTmpdir = `/var/folders/xx/${'a'.repeat(28)}/T/`
    const oldState = path.posix.join(guiTmpdir, 'muniment-subscriptions-XXXXXX', 'state')
    assert.throws(() => assertAttachSocketPath(platform, oldState), /maximum is 103 bytes/)
    const prefix = disposableProfilePrefix(platform, guiTmpdir)
    assert.equal(prefix, path.join('/tmp', 'muniment-subscriptions-'))
    const state = path.join(prefix + 'XXXXXX', 'state')
    assert.doesNotThrow(() => assertAttachSocketPath(platform, state))
    assert.ok(Buffer.byteLength(path.posix.join(state, 'muniment', 'attach-v1.sock')) < 104)
  })

  test(`the ${platform} socket preflight reserves a terminator and counts UTF-8 bytes`, () => {
    const suffixLength = Buffer.byteLength('/muniment/attach-v1.sock')
    const state = bytes => '/' + 'a'.repeat(bytes - suffixLength - 1)
    assert.doesNotThrow(() => assertAttachSocketPath(platform, state(103)))
    for (const bytes of [104, 105, 108]) {
      assert.throws(() => assertAttachSocketPath(platform, state(bytes)), new RegExp(`uses ${bytes} bytes`))
    }
    assert.throws(() => assertAttachSocketPath(platform, '/' + 'é'.repeat(39) + 'a'), /uses 104 bytes/)
  })
}

test('other platforms keep their temporary directory and socket rules', () => {
  const temporaryDirectory = path.join(os.tmpdir(), 'subscription-profile-test')
  for (const platform of ['linux', 'windows']) {
    assert.equal(path.dirname(disposableProfilePrefix(platform, temporaryDirectory)), temporaryDirectory)
    assert.doesNotThrow(() => assertAttachSocketPath(platform, '/' + 'a'.repeat(200)))
  }
})

test('the collector generates all platform cases and blocks missing or contradictory evidence', t => {
  const messages = []
  t.mock.method(console, 'error', text => messages.push(text))
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-collect-'))
  const output = path.join(root, 'out')
  const inputs = []
  try {
    for (const platform of platforms) {
      const directory = path.join(root, platform)
      inputs.push(directory)
      fs.mkdirSync(directory)
      const f = fixture()
      const proof = acceptance({ ...candidate, platform }, sourceSha, platform, f.result, f.transports, packageNames[platform])
      fs.writeFileSync(path.join(directory, 'release-acceptance.json'), JSON.stringify(proof))
      fs.writeFileSync(path.join(directory, `${platform}-subscription.json`), JSON.stringify({ ...f.result, transports: f.transports }))
      fs.writeFileSync(path.join(directory, `screenshot-${platform}-subscriptions.png`),
        Buffer.from(fs.readFileSync('test/e2e/fixtures/image-token.png.base64', 'utf8'), 'base64'))
    }
    assert.throws(() => collect(sourceSha, inputs[0], inputs))
    assert.equal(collect(sourceSha, output, inputs), 0)
    assert.deepEqual(messages, platforms.map(platform => `platform=${platform} status=passed\nreason="none"`))
    const combined = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    assert.equal(combined.cases.length, 72)
    assert.equal(Object.keys(combined.packages).length, 4)
    for (const platform of platforms) {
      assert.deepEqual(combined.cases.filter(item => item.platform === platform).map(item => item.feature), features)
      const log = fs.readFileSync(path.join(output, `${platform}-subscription.log`), 'utf8')
      assert.ok(features.every(feature => log.includes(`${feature}: passed\n`)))
    }
    const jobs = Object.fromEntries(platforms.map(platform => [platform, { result: 'success' }]))
    assert.equal(collect(sourceSha, output, inputs, jobs), 0)
    jobs.linux.result = 'failure'
    assert.equal(collect(sourceSha, output, inputs, jobs), 1)
    assert.ok(JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'))).cases
      .filter(item => item.platform === 'linux').every(item => item.status === 'blocked'))
    const linuxEvidence = path.join(inputs[0], 'linux-subscription.json')
    const incomplete = fixture()
    delete incomplete.result.features.mcp
    fs.writeFileSync(linuxEvidence, JSON.stringify({ ...incomplete.result, transports: incomplete.transports }))
    const partial = acceptance(candidate, sourceSha, 'linux', incomplete.result, incomplete.transports, packageNames.linux)
    fs.writeFileSync(path.join(inputs[0], 'release-acceptance.json'), JSON.stringify(partial))
    messages.length = 0
    assert.equal(collect(sourceSha, output, inputs), 1)
    assert.match(messages[0], /platform=linux status=blocked\nreason="The installed feature check did not finish/)
    const partialCombined = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    assert.deepEqual(partialCombined.cases.filter(item => item.status === 'blocked').map(item => [item.platform, item.feature]), [['linux', 'mcp']])
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
    assert.equal(combined.cases.length, 72)
    assert.ok(combined.cases.every(item => item.status === 'blocked' && item.reason === reason))
    assert.deepEqual(combined.packages, {})
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

for (const mode of ['success', 'wrong-selection', 'failed-reply', 'lost-context', 'new-thread', 'onboarding-only', 'disabled-send', 'hung-inventory', 'command-failed', 'progress-failed']) {
  test(`the installed webview probe checks ${mode}`, async () => {
    let selected = 0, picker = false, observed, clock = 0, hung = false
    const prompts = [], entries = [], progress = []
    const composer = { value: '', dispatchEvent() {}, getClientRects: () => [1] }
    const responses = () => entries.map(entry => ({ getClientRects: () => [1], querySelector: selector =>
      selector === '.assistant-markdown' ? { textContent: entry.text } : selector === '.provenance' ? {} : null }))
    const send = { textContent: 'Send', getAttribute: name => name === 'aria-label' ? 'Send' : mode === 'disabled-send' ? 'true' : null,
      getClientRects: () => [1], click: () => {
      prompts.push(composer.value)
      entries.push({ runId: `22222222-2222-4222-8222-22222222222${entries.length}`,
        phase: mode === 'failed-reply' ? 'failed' : 'complete', text: mode === 'lost-context' ? 'unknown' : nonce })
    } }
    const document = {
      head: { append() {} }, createElement: () => ({}),
      querySelector: selector => selector.startsWith('textarea') ?
        (mode === 'onboarding-only' && selector === 'textarea#composer-message' ? null : composer) : selector === '.model-chip'
        ? { click: () => { picker = true } } : picker ? {} : null,
      querySelectorAll: selector => selector === '.response' ? responses() : selector === 'button' ? [send]
        : models.map((model, index) => ({ dataset: { provider: 'muniment-router', model: `${model.family}/${model.id}` },
          click: () => { selected = index; picker = false } })),
    }
    const invoke = async (command, payload) => {
      if (command === 'subscription_probe_progress') {
        if (mode === 'progress-failed') throw new Error('PRIVATE TOKEN')
        progress.push(payload)
        return
      }
      if (command === 'local_mode_provider_inventory' && mode === 'command-failed') throw new Error('PRIVATE TOKEN AND REPLY')
      if (command === 'local_mode_provider_inventory' && mode === 'hung-inventory') { hung = true; return new Promise(() => {}) }
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
      document, Event: class {}, clearTimeout() {},
      setTimeout: (callback, ms) => { if (ms === 250 || hung) { hung = false; callback() } },
      Date: { now: () => { clock += 1000; return clock } },
    })
    assert.equal(observed.passed, mode === 'success')
    assert.ok(progress.length < 40)
    assert.ok(!JSON.stringify(progress).includes(nonce))
    assert.ok(!JSON.stringify({ progress, observed }).includes('PRIVATE'))
    if (mode === 'command-failed') assert.equal(progress.at(-1).errorClass, 'command-failed')
    if (mode === 'onboarding-only') {
      assert.equal(prompts.length, 0)
      assert.equal(progress.at(-1).stage, 'composer')
      assert.equal(progress.at(-1).errorClass, 'timeout')
    }
    if (mode === 'disabled-send') {
      assert.equal(prompts.length, 0)
      assert.equal(progress.at(-1).stage, 'send')
    }
    if (mode === 'hung-inventory') {
      assert.equal(progress.at(-1).stage, 'inventory')
      assert.equal(progress.at(-1).errorClass, 'command-timeout')
    }
    if (mode === 'success') {
      assert.deepEqual(progress.filter(row => row.stage === 'complete').map(row => row.turn), [0, 1, 2, 3])
      assert.equal(observed.turns.length, 4)
      assert.ok(prompts[0].includes(nonce))
      assert.ok(prompts.slice(1).every(prompt => !prompt.includes(nonce)))
      assert.equal(new Set(observed.turns.map(turn => turn.thread)).size, 1)
      assert.ok(!JSON.stringify(observed).includes(nonce))
    }
  })
}

for (const phase of ['features', 'restart', 'update', 'update-restart']) {
  test(`the ${phase} launch restores four replies without sending another chat`, async () => {
    let observed, checked = false
    const turns = fixture().result.turns.map(({ requested, expected, ...turn }) => turn)
    await vm.runInNewContext(fs.readFileSync('test/e2e/support/subscription-probe.js', 'utf8'), {
      window: { __MUNIMENT_SUBSCRIPTION_PLAN__: { phase, acceptance: true, turns, models, nonce },
        __munimentSubscriptionFeatures: async ({ plan }) => { checked = true; assert.equal(plan.phase, phase); return {} },
        __TAURI__: { core: { invoke: async (command, payload) => {
          if (command === 'subscription_probe_progress') return
          if (command === 'attach_listener_status') return { supervisor_running: true, connected: true }
          if (command === 'chat_current_thread') return thread
          if (command === 'subscription_probe_observed') { observed = payload; return }
          assert.fail('A restored launch must not select a model or send another chat.')
        } } } },
      document: { head: { append() {} }, createElement: () => ({}),
        querySelector: () => ({ getClientRects: () => [1] }),
        querySelectorAll: selector => {
          assert.equal(selector, '.response')
          return turns.map(() => ({ getClientRects: () => [1], querySelector: () => ({ textContent: nonce }), setAttribute() {} }))
        } },
      Event: class {}, setTimeout, clearTimeout,
    })
    assert.equal(checked, true)
    assert.equal(observed.passed, true)
    assert.deepEqual(observed.turns, turns)
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
      assert.equal(proof.cases.length, 18)
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

for (const arch of ['arm64', 'x64']) {
  test(`an overlong macOS ${arch} profile blocks the run before launch`, async t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-path-test-'))
    const profile = path.join(root, 'a'.repeat(110))
    const platform = `macos-${arch}`
    const previousPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    const previousArch = Object.getOwnPropertyDescriptor(process, 'arch')
    const previousDisposable = process.env.MUNIMENT_NATIVE_DISPOSABLE_USER
    const previousHome = process.env.HOME
    try {
      const keys = testKey()
      const packageName = packageNames[platform]
      const files = {
        candidateFile: JSON.stringify({ ...candidate, platform, asset: `nightly-${sourceSha}-macos-${packageName}` }),
        packageFile: bytes,
        signatureFile: signUpdaterBytes(bytes, keys.key, { fileName: packageName, version: '1.0.0' }),
        executable: 'The runner must not launch this file.',
        leasesFile: JSON.stringify([lease]),
        publicKeyFile: keys.publicText,
      }
      const inputs = {}
      for (const [name, content] of Object.entries(files)) {
        inputs[name] = path.join(root, name)
        fs.writeFileSync(inputs[name], content, { mode: 0o600 })
      }
      const output = path.join(root, 'out')
      fs.mkdirSync(output)
      fs.writeFileSync(path.join(output, 'release-acceptance.json'), JSON.stringify(build(fixture())))
      fs.mkdirSync(profile, { mode: 0o700 })
      t.mock.method(fs, 'mkdtempSync', prefix => {
        assert.equal(prefix, path.join('/tmp', 'muniment-subscriptions-'))
        return profile
      })
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
      Object.defineProperty(process, 'arch', { value: arch, configurable: true })
      process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = '1'
      process.env.HOME = '/Users/disposable'
      assert.equal(await run({ ...inputs, output, sourceSha, platform }), 1)
      const proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
      assert.ok(proof.cases.every(item => item.status === 'blocked' && item.installed === false))
      assert.match(proof.cases[0].reason, /socket path must be shorter than 104 bytes/)
      const log = fs.readFileSync(path.join(output, `${platform}-subscription.log`), 'utf8')
      assert.match(log, /step=profile\/attach-socket-path/)
      assert.match(log, /maximum is 103 bytes/)
      assert.equal(log.includes(lease.access), false)
      assert.equal(log.includes(lease.account_id), false)
      assert.equal(fs.existsSync(profile), false)
    } finally {
      t.mock.restoreAll()
      Object.defineProperty(process, 'platform', previousPlatform)
      Object.defineProperty(process, 'arch', previousArch)
      if (previousHome === undefined) delete process.env.HOME
      else process.env.HOME = previousHome
      if (previousDisposable === undefined) delete process.env.MUNIMENT_NATIVE_DISPOSABLE_USER
      else process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = previousDisposable
      fs.rmSync(root, { recursive: true, force: true })
    }
  })
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
    mutate?.({ packageFile, signatureFile, candidateFile, keyFile, keys, packageName, executable, leasesFile })
    const code = await run({
      candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform: 'linux',
      ...(publicKeyFile === true ? { publicKeyFile: keyFile } : publicKeyFile === null ? {} : { publicKeyFile }),
    })
    const proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json')))
    return { code, proof, reason: proof.cases[0].reason, log: fs.readFileSync(path.join(output, 'linux-subscription.log'), 'utf8'),
      evidence: JSON.parse(fs.readFileSync(path.join(output, 'linux-subscription.json'))) }
  } finally {
    if (previous === undefined) delete process.env.MUNIMENT_NATIVE_DISPOSABLE_USER
    else process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = previous
    fs.rmSync(root, { recursive: true, force: true })
  }
}

for (const condition of ['probe-source', 'probe-webdriver', 'chat-turn-count', 'chat-transport-count', 'chat-transport-file', 'none']) {
  test(`the installed runner preserves the ${condition} acceptance outcome`, { skip: !nativeLinux }, async () => {
    const f = fixture()
    const result = await identityRun(files => {
      const probe = { passed: true, source_sha: sourceSha, webdriver: false, turns: f.result.turns }
      if (condition === 'probe-source') probe.source_sha = 'b'.repeat(40)
      if (condition === 'probe-webdriver') probe.webdriver = true
      if (condition === 'chat-turn-count') probe.turns = []
      // The naming request finishes last and returns the nonce as its title.
      const receipts = [...f.transports, { ...f.transports[0], purpose: 'thread-name' }]
      if (condition === 'chat-transport-count') receipts.push(f.transports[0])
      const script = Buffer.from(`#!${process.execPath}
const fs = require('node:fs'), path = require('node:path')
const state = process.env.MUNIMENT_STATE_DIR
const plan = JSON.parse(fs.readFileSync(path.join(state, 'subscription-probe.json')))
const probe = ${JSON.stringify(probe)}
const receipts = ${JSON.stringify(receipts)}
const crypto = require('node:crypto')
for (const receipt of receipts) receipt.reply_sha256 = crypto.createHash('sha256').update(plan.nonce).digest('hex')
fs.writeFileSync(path.join(process.env.PI_CODING_AGENT_DIR, 'subscription-probe-transports.jsonl'),
  ${condition === 'chat-transport-file' ? "'PRIVATE REPLY AND CREDENTIAL'" : "receipts.map(row => JSON.stringify(row)).join('\\n')"})
// Stop after chat verification without a native feature test.
if (plan.phase !== 'chat') probe.passed = false
fs.writeFileSync(path.join(state, 'subscription-probe-result.json'), JSON.stringify(probe))
setInterval(() => {}, 1000)
`)
      fs.writeFileSync(files.packageFile, script)
      fs.writeFileSync(files.executable, script)
      fs.chmodSync(files.executable, 0o755)
      fs.writeFileSync(files.candidateFile, JSON.stringify({ ...candidate, sha256: hash(script) }))
      fs.writeFileSync(files.signatureFile, signUpdaterBytes(script, files.keys.key, { fileName: files.packageName, version: '1.0.0' }))
    })
    assert.equal(result.code, 1)
    assert.equal(result.evidence.condition, condition === 'none' ? undefined : condition)
    if (condition === 'none') {
      assert.match(result.log, /step=features\/verify-result/)
      assert.doesNotMatch(result.log, /acceptance condition failed/)
    } else {
      assert.match(result.log, /step=chat\/verify-result/)
      assert.ok(result.log.includes(`The subscription acceptance condition failed: ${condition}.`))
    }
    for (const value of [nonce, lease.access, lease.account_id, 'PRIVATE']) {
      assert.equal(JSON.stringify(result).includes(value), false)
    }
  })
}

for (const end of ['exit 42', 'kill -TERM $$']) {
  test(`the installed probe retains redacted process logs after ${end}`, { skip: !nativeLinux }, async () => {
    const result = await identityRun(files => {
      const script = Buffer.from(`#!/bin/sh\nprintf '%s\\n' 'app failed ${lease.access}' 'runtime failed ${lease.account_id}' >&2\n${end}\n`)
      fs.writeFileSync(files.packageFile, script)
      fs.writeFileSync(files.executable, script)
      fs.chmodSync(files.executable, 0o755)
      fs.writeFileSync(files.candidateFile, JSON.stringify({ ...candidate, sha256: hash(script) }))
      fs.writeFileSync(files.signatureFile, signUpdaterBytes(script, files.keys.key, { fileName: files.packageName, version: '1.0.0' }))
    })
    assert.equal(result.code, 1)
    assert.ok(result.proof.cases.every(item => item.status === 'blocked'))
    assert.match(result.log, /step=chat\/wait-result/)
    assert.match(result.log, /phase=chat/)
    assert.match(result.log, end === 'exit 42' ? /exit=42/ : /signal=SIGTERM/)
    assert.match(result.log, /app failed/)
    assert.match(result.log, /runtime failed/)
    assert.equal(result.log.includes(lease.access), false)
    assert.equal(result.log.includes(lease.account_id), false)
  })
}

test('a sandbox FATAL reaches blocked probe diagnostics after SIGTRAP', { skip: !nativeLinux }, async () => {
  const fatal = 'FATAL:content/browser/zygote_host/zygote_host_impl_linux.cc:129 No usable sandbox!'
  const result = await identityRun(files => {
    const script = Buffer.from(`#!/bin/sh
ulimit -c 0
mkdir -p "$MUNIMENT_STATE_DIR/browser"
printf '%s\\n' '${fatal} ${lease.account_id}' > "$MUNIMENT_STATE_DIR/browser/cef.log"
printf '%s\\n' '${fatal} ${lease.access}' >&2
kill -TRAP $$
`)
    fs.writeFileSync(files.packageFile, script)
    fs.writeFileSync(files.executable, script)
    fs.chmodSync(files.executable, 0o755)
    fs.writeFileSync(files.candidateFile, JSON.stringify({ ...candidate, sha256: hash(script) }))
    fs.writeFileSync(files.signatureFile, signUpdaterBytes(script, files.keys.key, { fileName: files.packageName, version: '1.0.0' }))
  })
  assert.equal(result.code, 1)
  assert.ok(result.proof.cases.every(item => item.status === 'blocked' && item.installed === false))
  assert.match(result.log, /step=chat\/wait-result\nphase=chat/)
  assert.match(result.log, /process exited before the result/)
  assert.match(result.log, /exit=none signal=SIGTRAP/)
  assert.match(result.log, /runtime: No runtime identity exists/)
  assert.match(result.log, /runtime tail:\nNo log exists/)
  assert.ok(result.log.includes(`app tail:\n${fatal}`))
  assert.ok(result.log.includes(`cef tail:\n${fatal}`))
  assert.equal(result.log.includes(lease.access), false)
  assert.equal(result.log.includes(lease.account_id), false)
})

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

test('the host blocks refresh tokens before remote dispatch and rejects stale passing proof after transport failure', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-host-failure-'))
  try {
    const options = { sourceSha, platform: 'linux', output: root, models: JSON.stringify(models), sshKey: 'key', knownHosts: 'host' }
    assert.equal(host({ ...options, leases: JSON.stringify([{ ...lease, refresh: 'private-refresh-token' }]),
      invoke: () => assert.fail('The host must not forward refresh tokens.') }), 1)
    assert.equal(fs.readFileSync(path.join(root, 'release-acceptance.json'), 'utf8').includes('private-refresh-token'), false)
    assert.equal(host({ ...options, leases: JSON.stringify([lease]), invoke: ({ output }) => {
      const f = fixture()
      fs.writeFileSync(path.join(output, 'release-acceptance.json'), JSON.stringify(build(f)))
      fs.writeFileSync(path.join(output, 'linux-subscription.json'), JSON.stringify({ ...f.result, transports: f.transports }))
      fs.writeFileSync(path.join(output, 'unapproved.log'), 'private-token')
      return { status: 1 }
    } }), 1)
    assert.ok(JSON.parse(fs.readFileSync(path.join(root, 'release-acceptance.json'))).cases.every(item => item.status === 'blocked'))
    assert.equal(fs.existsSync(path.join(root, 'unapproved.log')), false)
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
    const mapped = path.join(root, 'x64')
    assert.equal(host({
      sourceSha, platform: 'macos-x64', output: mapped, leases: JSON.stringify([lease]), models: JSON.stringify(models),
      sshKey: 'k', knownHosts: 'h', repository: 'owner/repo', token: 't',
      invoke: ({ platform, subscriptionPlatform, output: artifacts }) => {
        assert.equal(platform, 'macos')
        assert.equal(subscriptionPlatform, 'macos-x64')
        writeBlocked(artifacts, sourceSha, 'macos-x64', 'The native desktop-ci runner for this platform is unavailable.')
        return { status: 1 }
      },
    }), 1)
    assert.equal(JSON.parse(fs.readFileSync(path.join(mapped, 'release-acceptance.json'))).cases[0].platform, 'macos-x64')
    assert.equal(host({
      sourceSha, platform: 'macos-arm64', output: mapped, leases: JSON.stringify([lease]), models: JSON.stringify(models),
      sshKey: 'k', knownHosts: 'h', invoke: () => assert.fail('ARM64 must not use the Intel factory runner.'),
    }), 1)
    assert.match(JSON.parse(fs.readFileSync(path.join(mapped, 'release-acceptance.json'))).cases[0].reason, /macos-15/)
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

for (const platform of ['linux', 'macos', 'windows']) {
  test(`the subscription payload survives the ${platform} environment transport`, async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-transport-'))
    // Include shell syntax to catch quote loss and accidental evaluation.
    const leases = JSON.stringify([{ ...lease, account_id: 'fixture "account" $HOME `false` $(false) \\ \' é' }])
    const modelJson = JSON.stringify(models)
    let transported, calls = 0
    try {
      assert.deepEqual(runDesktopCi({
        sourceSha, platform, subscriptionPlatform: platform === 'macos' ? 'macos-x64' : platform,
        output: root, leases, models: modelJson, repository: 'owner/repo', token: 'fixture-token',
        sshKey: 'fixture-key', knownHosts: 'fixture-host', harnessSha: 'b'.repeat(40),
        spawnProcess(command, args, options) {
          calls += 1
          if (command === 'ssh') {
            assert.ok(args.at(-1).includes(`--ref '${'b'.repeat(40)}'`))
            assert.ok(options.input.includes(`MUNIMENT_E2E_SOURCE_SHA=${sourceSha}\n`))
            for (const value of [leases, lease.access, 'fixture-token', Buffer.from(leases).toString('base64')]) {
              assert.equal(args.join(' ').includes(value), false)
            }
            const envFile = path.join(root, 'dci_env')
            fs.writeFileSync(envFile, options.input, { mode: 0o600 })
            if (platform === 'windows') {
              // The Windows driver reads KEY=VALUE without shell expansion.
              transported = Object.fromEntries(options.input.trim().split('\n').map(line => {
                const separator = line.indexOf('=')
                return [line.slice(0, separator), line.slice(separator + 1)]
              }))
            } else {
              // Match the driver's shell-loading step, not a JavaScript parser.
              const loaded = spawnSync('/bin/bash', ['-c',
                'set -a; source "$ENV_FILE"; exec "$NODE" -e \'process.stdout.write(JSON.stringify(process.env))\''], {
                env: { PATH: process.env.PATH, ENV_FILE: envFile, NODE: process.execPath }, encoding: 'utf8', timeout: 10_000,
              })
              assert.equal(loaded.status, 0)
              assert.equal(loaded.stderr, '')
              transported = JSON.parse(loaded.stdout)
            }
            return { status: 0, stdout: '', stderr: '' }
          }
          assert.equal(command, 'bash')
          assert.equal(fs.readFileSync(args[1], 'utf8'), '')
          return { status: 0 }
        },
      }), { status: 0 })
      assert.equal(calls, 2)
      assert.equal(transported.MUNIMENT_SUBSCRIPTION_LEASES, undefined)
      assert.equal(decodeSubscriptionPayload(transported.MUNIMENT_SUBSCRIPTION_LEASES_BASE64), leases)
      assert.equal(decodeSubscriptionPayload(transported.MUNIMENT_SUBSCRIPTION_MODELS_BASE64), modelJson)
      let fetched = false
      const output = path.join(root, 'proof')
      assert.equal(await guest({
        sourceSha, platform: 'linux', output,
        encodedLeases: transported.MUNIMENT_SUBSCRIPTION_LEASES_BASE64,
        encodedModels: transported.MUNIMENT_SUBSCRIPTION_MODELS_BASE64,
        fetchRelease: () => { fetched = true; return { assets: [] } },
      }), 1)
      assert.equal(fetched, false)
      const proof = fs.readFileSync(path.join(output, 'release-acceptance.json'), 'utf8')
      assert.match(proof, /FACTORY_SUBSCRIPTION_LEASES/)
      assert.equal(proof.includes(lease.access), false)
      assert.equal(proof.includes(transported.MUNIMENT_SUBSCRIPTION_LEASES_BASE64), false)
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  })
}

test('the guest blocks malformed encoded payloads without exposing their values', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-transport-invalid-'))
  const previousError = console.error
  const logs = []
  console.error = value => logs.push(value)
  try {
    for (const field of ['encodedLeases', 'encodedModels']) {
      for (const encoded of ['fixture-secret!', 'W10', 'W11=', '', Buffer.from('{').toString('base64')]) {
        assert.equal(await guest({
          sourceSha, platform: 'linux', output: root, leases: '[]', models: '[]', [field]: encoded,
          fetchRelease: () => assert.fail('Invalid payloads must block before a download.'),
        }), 1)
        const proof = fs.readFileSync(path.join(root, 'release-acceptance.json'), 'utf8')
        assert.match(proof, field === 'encodedLeases' ? /FACTORY_SUBSCRIPTION_LEASES/ : /FACTORY_SUBSCRIPTION_MODELS/)
        assert.equal(proof.includes('fixture-secret'), false)
      }
    }
    assert.equal(logs.join('\n').includes('fixture-secret'), false)
  } finally {
    console.error = previousError
    fs.rmSync(root, { recursive: true, force: true })
  }
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

test('the guest distinguishes the Windows job self-test from package failures', () => {
  for (const message of [
    'The fixture did not reach its expected state.',
    'windows-job-test: status=1 signal=none error=none',
    'windows-job-test: status=none signal=SIGTERM error=spawnSync ETIMEDOUT',
    '',
  ]) {
    assert.equal(guestFailureReason('guest/windows-job-test', new Error(message)),
      'The Windows process job self-test failed.')
  }
  const failure = new Error('Native command failed.')
  const missingPackage = 'The signed nightly package or updater signature for this platform is missing.'
  for (const step of ['guest/nightly-assets', 'guest/updater-signature']) {
    assert.equal(guestFailureReason(step, failure), missingPackage)
  }
  assert.equal(guestFailureReason('guest/keychain-session', failure),
    'The installed macOS probe or disposable Keychain session failed.')
  assert.equal(guestFailureReason('guest/linux-sandbox', failure), sandboxRequirement)
  for (const step of ['guest/install', 'guest/unknown', '']) {
    assert.equal(guestFailureReason(step, failure), 'The native subscription guest failed.')
  }
  for (const message of [missingPackage,
    'Provide the FACTORY_SUBSCRIPTION_LEASES secret with access-only factory leases.',
    'Provide the FACTORY_SUBSCRIPTION_MODELS variable with four distinct supported model IDs.',
    'Run this check on the requested native platform and architecture.',
  ]) {
    assert.equal(guestFailureReason('guest/unknown', new Error(message)), message)
  }
  assert.equal(guestFailureReason('guest/windows-job-test', undefined), 'The Windows process job self-test failed.')
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

for (const mode of ['native', 'guest-blocked', 'linux', 'intel-node', 'intel-host', 'self-hosted', 'root', 'wrong-console', 'no-gui', 'no-uname']) {
  test(`the hosted ARM64 entry checks ${mode} before the signed guest`, async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-arm64-'))
    const env = { RUNNER_ARCH: 'ARM64', RUNNER_OS: 'macOS', GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' }
    if (mode === 'self-hosted') env.RUNNER_ENVIRONMENT = 'self-hosted'
    const commands = []
    let invoked = false
    try {
      const code = await hostedArm64({
        sourceSha, output: root, leases: JSON.stringify([lease]), models: JSON.stringify(models),
        repository: 'owner/repo', token: 'fixture-token', env,
        runtime: mode === 'linux' ? 'linux' : 'darwin', arch: mode === 'intel-node' ? 'x64' : 'arm64',
        uid: mode === 'root' ? 0 : 501,
        execute(command, args) {
          commands.push([command, args])
          assert.equal(JSON.stringify([command, args]).includes('fixture-token'), false)
          if (command === '/usr/bin/uname') return { status: mode === 'no-uname' ? 1 : 0, stdout: mode === 'intel-host' ? 'x86_64\n' : 'arm64\n' }
          if (command === '/usr/bin/stat') return { status: 0, stdout: mode === 'wrong-console' ? '502\n' : '501\n' }
          if (command === '/bin/launchctl') return { status: mode === 'no-gui' ? 1 : 0, stdout: 'GUI session\n' }
          assert.fail('Unexpected native command.')
        },
        invoke: async options => {
          invoked = true
          assert.equal(options.platform, 'macos-arm64')
          assert.equal(options.sourceSha, sourceSha)
          assert.equal(options.output, root)
          assert.equal(options.leases, JSON.stringify([lease]))
          assert.equal(options.models, JSON.stringify(models))
          assert.equal(options.repository, 'owner/repo')
          assert.equal(options.token, 'fixture-token')
          assert.deepEqual(commands, [
            ['/usr/bin/uname', ['-m']], ['/usr/bin/stat', ['-f', '%u', '/dev/console']], ['/bin/launchctl', ['print', 'gui/501']],
          ])
          return mode === 'guest-blocked' ? 1 : 0
        },
      })
      assert.equal(code, mode === 'native' ? 0 : 1)
      assert.equal(invoked, ['native', 'guest-blocked'].includes(mode))
      if (!invoked) {
        const proof = JSON.parse(fs.readFileSync(path.join(root, 'release-acceptance.json')))
        assert.ok(proof.cases.every(item => item.status === 'blocked' && item.platform === 'macos-arm64'))
        assert.match(proof.cases[0].reason, ['root', 'wrong-console', 'no-gui'].includes(mode) ? /native GUI session/ : /ARM64 runner/)
        assert.deepEqual(proof.packages, {})
      }
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  })
}

for (const platform of ['linux', 'windows', 'macos-x64']) {
  test(`the ${platform} subscription job uses the private release runner and its SSH channel`, () => {
    const workflow = fs.readFileSync('.github/workflows/subscriptions.yml', 'utf8')
    const job = workflow.split(`\n  ${platform}:\n`)[1]?.split(/\n  [\w-]+:\n/)[0]
    assert.ok(job, `The workflow must define the ${platform} subscription job.`)
    assert.deepEqual(job.match(/^    runs-on: .+$/gm), ['    runs-on: muniment-release'])
    assert.match(job, /run: node test\/e2e\/runner\/subscription-host\.mjs/)
    assert.match(job, /DESKTOP_CI_KNOWN_HOSTS: \$\{\{ vars\.DESKTOP_CI_KNOWN_HOSTS \}\}/)
    assert.match(job, /FACTORY_SUBSCRIPTION_LEASES: \$\{\{ secrets\.FACTORY_SUBSCRIPTION_LEASES \}\}/)
    assert.match(job, /FACTORY_SUBSCRIPTION_MODELS: \$\{\{ vars\.FACTORY_SUBSCRIPTION_MODELS \}\}/)
    // The private release runner supplies the SSH key through its environment.
    assert.doesNotMatch(workflow, /DESKTOP_CI_SSH_KEY/)
    const hostSource = fs.readFileSync('test/e2e/runner/subscription-host.mjs', 'utf8')
    assert.match(hostSource, /sshKey: process\.env\.DESKTOP_CI_SSH_KEY/)
  })
}

test('targeted subscription runs select one platform without granting full release proof', () => {
  const workflow = fs.readFileSync('.github/workflows/subscriptions.yml', 'utf8')
  const nightly = fs.readFileSync('.github/workflows/nightly.yml', 'utf8')
  assert.equal((workflow.match(/default: all/g) ?? []).length, 2)
  assert.match(workflow, /options: \[all, linux, windows, macos-arm64, macos-x64\]/)
  assert.match(workflow, /all\|linux\|windows\|macos\|macos-arm64\|macos-x64\) ;;/)
  assert.match(workflow, /Select a supported subscription platform.*exit 1/)
  for (const [jobName, selected] of [['linux', 'linux'], ['windows', 'windows'], ['macos-arm64', 'macos'], ['macos-x64', 'macos']]) {
    const job = workflow.split(`\n  ${jobName}:\n`)[1].split(/\n  [\w-]+:\n/)[0]
    const condition = job.match(/^    if: (.+)$/m)[1]
    const allowed = (platform, validation = 'success', cancelled = false) => Function(`return (${condition
      .replace('always()', 'true')
      .replace('cancelled()', JSON.stringify(cancelled))
      .replaceAll('needs.validate-platform.result', JSON.stringify(validation))
      .replaceAll('inputs.platform', JSON.stringify(platform))})`)()
    for (const platform of ['all', ...platforms, 'macos', '', 'invalid']) {
      assert.equal(allowed(platform), platform === 'all' || platform === selected || platform === jobName)
    }
    assert.match(job, /needs: .*validate-platform/)
    if (jobName !== 'linux') {
      assert.equal(allowed(selected, 'failure'), false)
      assert.equal(allowed(selected, 'success', true), false)
    }
  }
  const collect = workflow.split('\n  collect:\n')[1]
  assert.match(collect, /if: always\(\) && !cancelled\(\) && inputs.platform == 'all'/)
  const targeted = nightly.split('\n  targeted-release-acceptance:\n')[1].split('\n  proof:')[0]
  assert.match(targeted, /contains\(fromJSON\('\["linux","windows","macos"\]'\), github.event.inputs.platform\)/)
  assert.match(targeted, /source_sha: \$\{\{ needs.prepare.outputs.source_sha \}\}/)
  assert.match(targeted, /platform: \$\{\{ github.event.inputs.platform \}\}/)
  assert.match(targeted, /uses: \.\/\.github\/workflows\/subscriptions.yml/)
  assert.match(nightly, /ref: context.sha/)
  const proof = nightly.split('\n  proof:\n')[1]
  assert.match(proof, /needs.release-acceptance.result == 'success'/)
  assert.match(proof, /needs.publish.result == 'success'/)
  assert.doesNotMatch(proof, /targeted-release-acceptance/)
})

test('targeted subscription dispatch keeps full release acceptance exclusive to all platforms', () => {
  const workflow = fs.readFileSync('.github/workflows/subscriptions.yml', 'utf8')
  const dispatch = workflow.split('  workflow_dispatch:')[1].split('  workflow_call:')[0]
  assert.match(dispatch, /platform:\s+description:.*\n\s+type: choice\n\s+default: all\n\s+options: \[all, linux, windows, macos-arm64, macos-x64\]/)
  const reusable = workflow.split('  workflow_call:')[1].split('\n#')[0]
  assert.match(reusable, /platform:\s+description:.*\n\s+type: string\n\s+default: all/)
  for (const selected of ['', 'all', ...platforms, 'macos', 'invalid']) {
    for (const jobName of [...platforms, 'collect']) {
      const job = workflow.split(`\n  ${jobName}:\n`)[1].split(/\n  [\w-]+:\n/)[0]
      const condition = job.match(/^    if: (.*)$/m)[1]
      const enabled = vm.runInNewContext(condition.replaceAll('needs.validate-platform.result', "needs['validate-platform'].result"), { inputs: { platform: selected },
        needs: { 'validate-platform': { result: 'success' } }, always: () => true, cancelled: () => false })
      assert.equal(enabled, selected === 'all' || jobName === selected || (selected === 'macos' && jobName.startsWith('macos-')),
        `${selected}: ${jobName}`)
    }
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
  const armJob = workflow.split('  macos-arm64:\n')[1].split('\n  macos-x64:')[0]
  assert.match(armJob, /runs-on: macos-15\n/)
  assert.match(armJob, /run: node test\/e2e\/runner\/subscription-macos-arm64\.mjs/)
  assert.match(armJob, /GH_TOKEN: \$\{\{ github\.token \}\}/)
  assert.equal(armJob.includes('subscription-host.mjs'), false)
  assert.equal(armJob.includes('DESKTOP_CI_'), false)
  assert.match(armJob, /if: always\(\)/)
  assert.match(armJob, /uses: .\/.github\/actions\/store-artifact/)
  const empty = fs.mkdtempSync(path.join(os.tmpdir(), 'empty-subscription-evidence-'))
  try {
    assert.throws(() => upload({ repository: 'munimentai/muniment', run: 1, attempt: 1, source: sourceSha },
      'subscription-macos-arm64', empty, { put() { assert.fail('Empty evidence must not upload.') } }), /Invalid artifact file count/)
  } finally { fs.rmSync(empty, { recursive: true, force: true }) }
  assert.match(workflow, /name: release-acceptance/)
  assert.match(workflow, /test "\$COLLECT_STATUS" = 0/)
  assert.match(workflow, /SUBSCRIPTION_JOB_RESULTS: \$\{\{ toJSON\(needs\) \}\}/)
  assert.match(workflow, /installed release acceptance checks passed on all four platforms/)
  assert.equal((workflow.match(/ref: \$\{\{ github.sha \}\}/g) ?? []).length, 5)
  assert.match(workflow, /HARNESS_SHA: \$\{\{ github.sha \}\}/)
  assert.equal((workflow.match(/SOURCE_SHA: \$\{\{ inputs.source_sha \}\}/g) ?? []).length, 5)
  assert.match(nightly, /uses: \.\/\.github\/workflows\/subscriptions.yml/)
  assert.match(nightly, /needs\.release-acceptance\.result == 'success'/)
  const acceptanceJob = nightly.split('  release-acceptance:\n')[1].split('\n  proof:')[0]
  assert.match(acceptanceJob, /needs: \[prepare, build, publish, linux-e2e, windows-e2e, macos-e2e\]/)
  assert.match(acceptanceJob, /source_sha: \$\{\{ needs.prepare.outputs.source_sha \}\}/)
  assert.match(acceptanceJob, /secrets: inherit/)
  assert.match(acceptanceJob, /github.event.inputs.platform == 'all'/)
  assert.doesNotMatch(acceptanceJob, /outputs.reuse|continue-on-error/)
  const condition = acceptanceJob.match(/if: >-\n((?:      .+\n)+)/)[1].trim()
  const allowed = (platform, prepare = 'success', cancelled = false) => Function(`return (${condition
    .replace('cancelled()', JSON.stringify(cancelled))
    .replaceAll('needs.prepare.result', JSON.stringify(prepare))
    .replaceAll('github.event.inputs.platform', JSON.stringify(platform))})`)()
  assert.equal(allowed('all'), true)
  assert.equal(allowed(''), true)
  assert.equal(allowed('all', 'failure'), false)
  assert.equal(allowed('all', 'success', true), false)
  for (const platform of ['linux', 'windows', 'macos']) assert.equal(allowed(platform), false)
  const proofJob = nightly.split('  proof:\n')[1]
  for (const gate of ['build', 'publish', 'linux-e2e', 'windows-e2e', 'macos-e2e', 'release-acceptance']) {
    assert.ok(proofJob.includes(`needs.${gate}.result == 'success'`))
  }
  assert.match(workflow, /rm -rf "\$RUNNER_TEMP\/subscription-inputs"/)
  assert.equal((workflow.match(/name: Clear platform output/g) ?? []).length, 4)
  assert.ok((workflow.match(/if: always\(\)/g) ?? []).length >= 6)
  assert.match(workflow, /needs: \[linux, windows, macos-arm64, macos-x64\]/)
  assert.equal(nightly.includes('subscription-host.mjs'), false)
  assert.equal(nightly.includes('FACTORY_SUBSCRIPTION_LEASES'), false)
  assert.equal(workflow.includes('echo $FACTORY_SUBSCRIPTION_LEASES'), false)
  const hostSource = fs.readFileSync('test/e2e/runner/subscription-host.mjs', 'utf8')
  assert.match(hostSource, /harnessSha: process\.env\.HARNESS_SHA \?\? process\.env\.SOURCE_SHA/)
  assert.equal(hostSource.includes('console.log(leases'), false)
  assert.equal(hostSource.includes('console.error(leases'), false)
})

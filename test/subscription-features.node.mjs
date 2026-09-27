import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import vm from 'node:vm'
import https from 'node:https'
import { spawnSync } from 'node:child_process'
import { updateFixture } from './e2e/runner/subscription-update.mjs'
import { awaitUpdateResult, verifyUpdateResult } from './e2e/runner/subscriptions.mjs'
import { featureChecks } from './e2e/support/subscription-acceptance.mjs'

const nonce = 'MUNIMENT-' + 'a'.repeat(32)
const fileNonce = 'MUNIMENT-' + 'b'.repeat(32)
const mcpNonce = 'MUNIMENT-' + 'c'.repeat(32)
async function probe(failure) {
  const plan = { nonce, fileNonce, mcpNonce, mcpReceipt: '/tmp/mcp-receipt.json', models: [{ family: 'openai', id: 'route-model' }, { family: 'openai', id: 'model' }], fixtureFile: '/tmp/fixture.txt',
    fixtureDirectory: '/tmp', mcpCommand: 'node', mcpScript: '/tmp/mcp.mjs',
    turns: [{ thread: 'thread', run: 'chat' }] }
  const state = { content: fileNonce, revision: 'first', profile: '', projects: {},
    accounts: [{ id: 'one', family: 'openai', source: 'account', requests: 2, active: 0, errors: 0, label: 'One', weight: 1 },
      { id: 'two', family: 'openai', source: 'account', requests: failure === 'single-account' ? 0 : 2, active: 0, errors: 0, label: 'Two', weight: 1 }],
    entries: [{ runId: 'chat', text: nonce, phase: 'complete' }], commands: [] }
  for (const account of state.accounts) { account.enabled = true; account.models = ['route-model', 'model'] }
  if (failure === 'unequal-shares') { state.accounts[0].requests = 3; state.accounts[1].requests = 1 }
  if (failure === 'active-reservation') state.accounts[0].active = 1
  let terminalOpen = false, terminalRead = false, prompt = ''
  const composer = {}
  const document = { querySelector: selector => selector.startsWith('textarea') ? composer
    : selector.startsWith('[role="switch"]') ? { click: () => { state.mcpSelected = true },
      getAttribute: () => String(state.mcpSelected === true) } : { click() {} }, querySelectorAll: () => [{
    getAttribute: () => 'Send', click() {
      state.entries.push({ runId: `tool-${state.entries.length}`, text: failure === 'replayed-chat-token' ? nonce : prompt.includes('MCP') ? mcpNonce : fileNonce, phase: 'complete', receipt: {
        tools: [{ name: prompt.includes('MCP') ? 'mcp__acceptance_token' : 'read', calls: failure === 'no-tool' ? 0 : 1, failed: 0 }],
      } })
    },
  }] }
  const invoke = async (command, data = {}) => {
    state.commands.push(command)
    if (command === failure) throw new Error('private provider error')
    if (command === 'subscription_probe_update') return
    if (command === 'model_router_settings') return structuredClone({ accounts: state.accounts, routes: [], fallback: state.fallback ?? null, min_confidence: 0.6 })
    if (command === 'model_router_update_account') {
      if (data.label !== undefined && !data.label) throw new Error('Invalid label.')
      Object.assign(state.accounts.find(account => account.id === data.id), data)
      return
    }
    if (command === 'model_router_save_routes') {
      if (data.fallback?.includes('nonexistent')) throw new Error('Unknown model.')
      state.fallback = data.fallback
      return
    }
    if (command === 'model_router_test_route') {
      if (!data.sample) throw new Error('Empty sample.')
      return { model: state.fallback, eligible_models: [...new Set(state.accounts.filter(account => account.enabled)
        .flatMap(account => account.models.map(id => `openai/${id}`)))], fallback_reason: 'No classifier.' }
    }
    if (command === 'workspace_folders') return [{ path: '/tmp' }]
    if (command === 'workspace_file_action') return ['/tmp/artifact.html']
    if (command === 'workspace_read_text') return { content: data.path === '/tmp/artifact.html' ? '' : state.content, revision: state.revision }
    if (command === 'workspace_save_text') {
      if (data.path === '/tmp/artifact.html') { state.html = data.content; return }
      if (data.revision !== state.revision) throw new Error('Stale revision.')
      state.content = data.content
      state.revision = 'second'
      return
    }
    if (command === 'project_create') { state.projects.project = data.name; return 'project' }
    if (command === 'project_list') return { projects: state.projects }
    if (command === 'project_rename') { state.projects[data.projectId] = data.name; return }
    if (command === 'memory_profile_read') return state.profile
    if (command === 'memory_profile_save') { state.profile = data.content; return }
    if (command === 'agent_save') { state.agent = { id: 'agent', ...data.agent }; return structuredClone(state.agent) }
    if (command === 'agent_list') return { agents: state.agent ? [state.agent] : [] }
    if (command === 'agent_delete') { state.agent = null; return }
    if (command === 'artifact_from_file') return { id: 'artifact' }
    if (command === 'artifact_read') return { html: state.html }
    if (command === 'artifact_edit') { state.artifactName = data.name; return }
    if (command === 'artifact_list') return [{ id: 'artifact', name: state.artifactName }]
    if (command === 'browser_view') return
    if (command === 'browser_command') {
      if (data.request.action === 'navigate') throw new Error('Unsafe URL.')
      if (data.request.action === 'snapshot') {
        state.snapshots = (state.snapshots ?? 0) + 1
        if (failure === 'loading-forever' || (failure === 'loading-once' && state.snapshots === 1)) throw 'The page is still loading.'
        if (failure === 'loading-error-once' && state.snapshots === 1) throw new Error('The page is still loading.')
        if (failure === 'snapshot-failed') throw new Error('The snapshot failed.')
        return JSON.stringify({ title: 'Docs', text: 'The local documentation page.', url: 'http://127.0.0.1:1234/docs/' })
      }
      return {}
    }
    if (command === 'terminal_start') { terminalOpen = true; return 'terminal' }
    if (command === 'terminal_write') return
    if (command === 'terminal_close') { terminalOpen = false; return }
    if (command === 'terminal_read') {
      if (!terminalOpen) throw new Error('Closed terminal.')
      const text = terminalRead ? '' : `echo ${nonce}\r\n${failure === 'echo-only' ? '' : nonce + '\r\n'}`
      terminalRead = true
      return { bytes: [...Buffer.from(text)] }
    }
    if (command === 'chat_thread_open') return structuredClone({ entries: state.entries })
    if (command === 'extend_command') {
      if (data.action === 'test') return { status: 'connected', tools: failure === 'empty-tools' ? [] : [{ name: 'acceptance_token' }] }
      return {}
    }
    if (command === 'chat_current_thread') return failure === 'lost-thread' ? 'different' : 'thread'
    if (command === 'local_mode_provider_inventory') return { default_provider: 'muniment-router', default_model: 'openai/model' }
    throw new Error(`Unexpected command: ${command}`)
  }
  const context = { window: {}, document, URL }
  vm.runInNewContext(fs.readFileSync('test/e2e/support/subscription-features.js', 'utf8'), context)
  const wait = async predicate => {
    for (let attempt = 0; attempt < 10; attempt++) {
      const result = await predicate()
      if (result) return result
    }
    throw new Error('The check timed out.')
  }
  const run = phase => context.window.__munimentSubscriptionFeatures({ plan: { ...plan, phase }, invoke, wait,
    setValue: (_element, value) => { prompt = value }, turns: plan.turns })
  const initial = { ...await run('chat'), ...await run('features') }
  const restart = await run('restart')
  await assert.rejects(run('update'))
  const updated = failure === 'subscription_probe_update' ? { 'signed-update': [] } : await run('update-restart')
  return { initial: JSON.parse(JSON.stringify({ ...initial, 'signed-update': updated['signed-update'] })),
    restart: JSON.parse(JSON.stringify(restart)), state }
}

test('the installed feature probe exercises commands and validates their results', async () => {
  const result = await probe()
  for (const [feature, checks] of Object.entries(featureChecks)) {
    if (feature === 'local-startup') continue
    assert.deepEqual({ ...result.initial, ...result.restart }[feature], feature === 'signed-update' ? checks.slice(0, -1) : checks, feature)
  }
  assert.equal(result.state.content, `${fileNonce}\nEdited\n`)
  assert.equal(result.state.profile, '')
  assert.equal(result.state.projects.project, 'Renamed acceptance project')
  assert.equal(result.state.entries.length, 3)
  assert.equal(result.state.mcpSelected, true)
})

for (const [failure, feature] of [
  ['single-account', 'account-balancing'], ['unequal-shares', 'account-balancing'],
  ['active-reservation', 'account-balancing'], ['model_router_update_account', 'settings'],
  ['model_router_test_route', 'routing'], ['workspace_save_text', 'files'],
  ['project_create', 'projects'], ['memory_profile_save', 'memory'], ['echo-only', 'terminal'],
  ['no-tool', 'tools'], ['replayed-chat-token', 'tools'], ['empty-tools', 'mcp'], ['extend_command', 'mcp'],
  ['subscription_probe_update', 'signed-update'], ['lost-thread', 'restart-persistence'],
  ['agent_save', 'agents'], ['artifact_read', 'artifacts'], ['browser_command', 'browser'],
]) test(`the installed probe blocks ${feature} after ${failure}`, async () => {
  const result = await probe(failure)
  assert.deepEqual({ ...result.initial, ...result.restart }[feature], [])
  assert.equal(JSON.stringify(result.initial).includes('private provider error'), false)
  if (feature !== 'projects') assert.deepEqual(result.initial.projects, featureChecks.projects)
})

for (const failure of ['loading-once', 'loading-error-once']) {
  test(`the browser probe retries ${failure}`, async () => {
    const result = await probe(failure)
    assert.deepEqual(result.initial.browser, featureChecks.browser)
    assert.equal(result.state.snapshots, 2)
  })
}

for (const [failure, attempts] of [['loading-forever', 10], ['snapshot-failed', 1]]) {
  test(`the browser probe blocks ${failure} and closes the view`, async () => {
    const result = await probe(failure)
    assert.deepEqual(result.initial.browser, [])
    assert.equal(result.state.snapshots, attempts)
    assert.equal(result.state.commands.filter(command => command === 'browser_view').length, 2)
  })
}

test('the update probe requires a new process, restored profile, and the candidate payload', async () => {
  const turns = [{ thread: 'thread', run: 'chat' }]
  const valid = { pid: 200, update_parent_pid: 100, passed: true, phase: 'update-restart', source_sha: 'a'.repeat(40), webdriver: false, turns,
    features: { 'signed-update': featureChecks['signed-update'].slice(0, -1) } }
  let verified = 0
  const verify = result => verifyUpdateResult(result, 100, valid.source_sha, turns, () => { verified++ })
  assert.deepEqual(verify(structuredClone(valid)).features['signed-update'], featureChecks['signed-update'])
  assert.equal(verified, 1)
  for (const change of [
    { passed: false }, { passed: 'true' }, { phase: 'update' }, { pid: 100 }, { pid: 0 },
    { pid: 2.5 }, { pid: '200' }, { pid: 2147483648 }, { update_parent_pid: 200 }, { update_parent_pid: undefined },
    { source_sha: 'b'.repeat(40) }, { webdriver: true }, { turns: [] },
    { features: { 'signed-update': [] } },
    { features: { 'signed-update': featureChecks['signed-update'].slice(0, 3) } },
  ]) assert.throws(() => verify({ ...structuredClone(valid), ...change }))
  assert.equal(verified, 1)
  const changedPayload = structuredClone(valid)
  assert.throws(() => verifyUpdateResult(changedPayload, 100, valid.source_sha, turns, () => {
    throw new Error('The candidate digest does not match.')
  }))
  assert.equal(changedPayload.features['signed-update'].includes('candidate-digest-verified'), false)
})

for (const mode of ['failed-install', 'returned-without-restart']) {
  test(`the installed webview blocks ${mode}`, async () => {
    const turns = Array.from({ length: 4 }, (_, index) => ({ thread: 'thread', run: `chat-${index}` }))
    let observed, installs = 0
    const context = { window: { __MUNIMENT_SUBSCRIPTION_PLAN__: { phase: 'update', acceptance: true, turns, nonce },
      __TAURI__: { core: { invoke: async (command, data) => {
        if (command === 'attach_listener_status') return { supervisor_running: true, connected: true }
        if (command === 'chat_current_thread') return 'thread'
        if (command === 'subscription_probe_update') {
          installs++
          if (mode === 'failed-install') throw new Error('The installation failed.')
          return
        }
        if (command === 'subscription_probe_observed') { observed = data; return }
        assert.fail(`Unexpected command: ${command}`)
      } } } },
      document: { querySelector: () => ({ getClientRects: () => [1] }),
        querySelectorAll: () => turns.map(() => ({ getClientRects: () => [1], querySelector: () => ({ textContent: nonce }) })) },
    }
    vm.createContext(context)
    vm.runInContext(fs.readFileSync('test/e2e/support/subscription-features.js', 'utf8'), context)
    await vm.runInContext(fs.readFileSync('test/e2e/support/subscription-probe.js', 'utf8'), context)
    assert.equal(installs, 1)
    assert.equal(observed.passed, false)
    assert.deepEqual(Object.keys(observed.features), [])
  })
}

test('the update wait blocks a missing relaunch and reports a failed installation', async () => {
  let clock = 0
  const options = { now: () => clock, wait: async ms => { clock += ms }, timeout: 1000 }
  await assert.rejects(awaitUpdateResult(() => false, options), /did not restart/)
  assert.equal(clock, 1000)
  const failed = { passed: false, pid: 100, features: {} }
  assert.equal(await awaitUpdateResult(() => failed, options), failed)
  assert.throws(() => verifyUpdateResult(failed, 100, 'a'.repeat(40), [], () => assert.fail('Unexpected payload check.')))
  clock = 0
  const restarted = { passed: true, pid: 200 }
  assert.equal(await awaitUpdateResult(() => clock >= 500 && restarted, options), restarted)
  assert.equal(clock, 500)
})

test('the installed probe uses production update commands and retains Windows installer selection', () => {
  const probe = fs.readFileSync('src-tauri/src/subscription_probe.rs', 'utf8')
  const production = fs.readFileSync('src-tauri/src/app_update.rs', 'utf8')
  assert.match(probe, /app_update_prepare\(app\.clone\(\), state\.clone\(\)\)/)
  assert.match(probe, /app_update_install\(app\.clone\(\), activity, state\)/)
  assert.match(probe, /mark_active_run\(\)/)
  assert.doesNotMatch(probe, /\.updater_builder\(\)/)
  assert.match(production, /builder\.target\(windows_update_target\(\)\?\)/)
  assert.match(production, /ready\.0\.install\(&ready\.1\)/)
  assert.match(production, /app\.restart\(\)/)
  for (const file of ['per-user.wxs', 'per-machine.wxs']) {
    const installer = fs.readFileSync(`src-tauri/windows/${file}`, 'utf8')
    assert.match(installer, /AUTOLAUNCHAPP AND NOT \(REMOVE = "ALL"\) AND \(NOT Installed OR REINSTALL\)/)
  }
})

test('the MCP fixture returns a token only through the declared tool', () => {
  const requests = [
    { id: 1, method: 'initialize', params: { protocolVersion: '2024-11-05' } },
    { method: 'notifications/initialized' }, { id: 2, method: 'tools/list' },
    { id: 3, method: 'tools/call', params: { name: 'acceptance_token' } },
    { id: 4, method: 'tools/call', params: { name: 'unknown' } },
  ]
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-mcp-test-'))
  const receipt = path.join(root, 'receipt.json')
  try {
    const result = spawnSync(process.execPath, ['test/e2e/support/subscription-mcp.mjs', nonce, receipt], {
      input: requests.map(request => JSON.stringify({ jsonrpc: '2.0', ...request })).join('\n') + '\n',
      encoding: 'utf8', timeout: 10_000,
    })
    assert.equal(result.status, 0)
    const replies = result.stdout.trim().split('\n').map(JSON.parse)
    assert.equal(replies.length, 4)
    assert.equal(replies[1].result.tools[0].name, 'acceptance_token')
    assert.equal(replies[2].result.content[0].text, nonce)
    assert.equal(replies[3].error.code, -32601)
    assert.equal(result.stderr, '')
    assert.deepEqual(JSON.parse(fs.readFileSync(receipt, 'utf8')), { token: nonce })
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

for (const [platform, target] of Object.entries({ linux: 'linux-x86_64', windows: 'windows-x86_64-msi-user',
  'macos-arm64': 'darwin-aarch64', 'macos-x64': 'darwin-x86_64' })) test(`the ${platform} update fixture serves pinned bytes and a damaged copy over loopback TLS`, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-update-test-'))
  let server
  try {
    const bytes = Buffer.from('signed package')
    await assert.rejects(updateFixture(root, Buffer.alloc(0), 'signature', '1.0.0', 'linux'))
    await assert.rejects(updateFixture(root, bytes, '', '1.0.0', 'linux'))
    await assert.rejects(updateFixture(root, bytes, 'signature', 'invalid', 'linux'))
    for (const platform of ['unknown', 'toString', '__proto__', undefined]) {
      await assert.rejects(updateFixture(root, bytes, 'signature', '1.0.0', platform))
    }
    assert.deepEqual(fs.readdirSync(root), [])
    server = await updateFixture(root, bytes, 'signature', '1.0.0', platform)
    const get = url => new Promise((resolve, reject) => {
      https.get(url, { rejectUnauthorized: false }, response => {
        const chunks = []
        response.on('data', chunk => chunks.push(chunk))
        response.on('end', () => resolve(Buffer.concat(chunks)))
      }).on('error', reject)
    })
    const manifest = JSON.parse((await get(server.url)).toString())
    assert.equal(new URL(server.url).hostname, '127.0.0.1')
    assert.equal(manifest.version, '1.0.0')
    assert.deepEqual(Object.keys(manifest.platforms), [target])
    assert.equal(manifest.url, undefined)
    assert.equal(manifest.platforms[target].signature, 'signature')
    assert.deepEqual(await get(manifest.platforms[target].url), bytes)
    const damaged = await get(new URL('/tampered', server.url))
    assert.equal(damaged.length, bytes.length)
    assert.notDeepEqual(damaged, bytes)
  } finally {
    await server?.close()
    fs.rmSync(root, { recursive: true, force: true })
  }
})

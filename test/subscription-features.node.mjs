import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import vm from 'node:vm'
import https from 'node:https'
import { spawnSync } from 'node:child_process'
import { updateFixture } from './e2e/runner/subscription-update.mjs'
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
      if (data.request.action === 'snapshot') return JSON.stringify({ title: 'Docs', text: 'The local documentation page.', url: 'http://127.0.0.1:1234/docs/' })
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
  return { initial: JSON.parse(JSON.stringify(initial)), restart: JSON.parse(JSON.stringify(restart)), state }
}

test('the installed feature probe exercises commands and validates their results', async () => {
  const result = await probe()
  for (const [feature, checks] of Object.entries(featureChecks)) {
    if (feature === 'local-startup') continue
    assert.deepEqual({ ...result.initial, ...result.restart }[feature], checks, feature)
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

test('the update fixture serves pinned bytes and a damaged copy over loopback TLS', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-update-test-'))
  let server
  try {
    const bytes = Buffer.from('signed package')
    await assert.rejects(updateFixture(root, Buffer.alloc(0), 'signature', '1.0.0'))
    await assert.rejects(updateFixture(root, bytes, '', '1.0.0'))
    await assert.rejects(updateFixture(root, bytes, 'signature', 'invalid'))
    assert.deepEqual(fs.readdirSync(root), [])
    server = await updateFixture(root, bytes, 'signature', '1.0.0')
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
    assert.equal(manifest.signature, 'signature')
    assert.deepEqual(await get(manifest.url), bytes)
    const damaged = await get(new URL('/tampered', server.url))
    assert.equal(damaged.length, bytes.length)
    assert.notDeepEqual(damaged, bytes)
  } finally {
    await server?.close()
    fs.rmSync(root, { recursive: true, force: true })
  }
})

import './subscription-feature-evidence.node.mjs'
import { test } from 'node:test'
import xterm from '@xterm/xterm'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import vm from 'node:vm'
import https from 'node:https'
import { spawnSync } from 'node:child_process'
import { corePath } from '../scripts/muniment-core.mjs'
import { updateFixture } from './e2e/runner/subscription-update.mjs'
import { awaitUpdateResult, verifyUpdateResult, verifyMcpReceipt } from './e2e/runner/subscriptions.mjs'
import { featureChecks, featureFailure, interruptionErrors } from './e2e/support/subscription-acceptance.mjs'

const nonce = 'MUNIMENT-' + 'a'.repeat(32)
const terminalResponse = `${nonce}-terminal`
const terminalCommand = `echo ${nonce}\\-terminal\r`
const fileNonce = 'MUNIMENT-' + 'b'.repeat(32)
const mcpNonce = 'MUNIMENT-' + 'c'.repeat(32)
const terminalFailure = (reason, text = '') => ['failed', 'check', reason,
  `bytes-${Buffer.byteLength(text)}`, `received-${text.length > 0}`, `nonce-${text.includes(nonce)}`, `vt-${text.includes('\x1b')}`]
const mcpApproval = {
  gateId: 'mcp-gate', kind: 'select',
  title: 'MCP: extend-release-acceptance wants to run acceptance_token\n\nArguments:\n{}\n\nAllow server for this session permits all tools and arguments on this server until reload or session/branch change. Other security and UI consent checks still apply.',
  options: ['Allow once', 'Allow for session', 'Allow server for this session', 'Deny'],
}
async function probe(failure, fault = () => {}) {
  const plan = { platform: 'linux', nonce, fileNonce, mcpNonce, mcpReceipt: '/tmp/mcp-receipt.json', models: [{ family: 'openai', id: 'route-model' }, { family: 'openai', id: 'model' }], fixtureFile: '/tmp/fixture.txt',
    fixtureDirectory: '/tmp', mcpCommand: 'node', mcpScript: '/tmp/mcp.mjs',
    turns: [{ thread: 'thread', run: 'chat' }] }
  const state = { content: fileNonce, revision: 'first', profile: '', projects: {},
    accounts: [{ id: 'one', family: 'openai', source: 'account', requests: 2, active: 0, errors: 0, label: 'One', weight: 1 },
      { id: 'two', family: 'openai', source: 'account', requests: failure === 'single-account' ? 0 : 2, active: 0, errors: 0, label: 'Two', weight: 1 }],
    entries: [{ runId: 'chat', text: nonce, phase: 'complete' }], commands: [], prompts: [] }
  for (const account of state.accounts) { account.enabled = true; account.models = ['route-model', 'model'] }
  if (failure === 'unequal-shares') { state.accounts[0].requests = 3; state.accounts[1].requests = 1 }
  if (failure === 'active-reservation') state.accounts[0].active = 1
  fault('setup', plan, state)
  let terminalOpen = false, terminalRead = false, prompt = ''
  const composer = {}
  const document = { querySelector: selector => {
    const result = { value: selector.startsWith('textarea') ? composer
      : selector.startsWith('[role="switch"]') ? { click: () => { fault('mcp-toggle'); state.mcpSelected = true },
        getAttribute: () => String(state.mcpSelected === true) } : { click() {} } }
    fault('query-selector', result, state)
    return result.value
  }, querySelectorAll: () => {
    fault('query-buttons', undefined, state)
    return [{
      getAttribute: () => 'Send', click() {
        fault('send-click', undefined, state)
        state.prompts.push(prompt)
        if (prompt.includes('MCP')) fault('mcp-tool-turn')
        const entry = { runId: `tool-${state.entries.length}`, text: failure === 'replayed-chat-token' ? nonce : prompt.includes('MCP') ? mcpNonce : fileNonce, phase: 'complete', receipt: {
          tools: [{ name: prompt.includes('MCP') ? 'mcp__acceptance_token' : 'read', calls: failure === 'no-tool' ? 0 : 1, failed: 0 }],
        } }
        fault(prompt.includes('MCP') ? 'mcp-reply' : 'tools-reply', entry)
        state.entries.push(entry)
      },
    }]
  } }
  const execute = async (command, data = {}) => {
    state.commands.push(command)
    fault(command, data, state)
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
    if (command === 'workspace_folders') return failure === 'empty-folders' ? [] : [{ path: '/tmp' }]
    if (command === 'workspace_file_action') return failure === 'empty-files' ? [] : ['/tmp/artifact.html']
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
    if (command === 'terminal_write') { state.terminalWrites = [...(state.terminalWrites ?? []), data.data]; return }
    if (command === 'terminal_close') { terminalOpen = failure === 'unclosed-terminal'; return }
    if (command === 'terminal_read') {
      if (!terminalOpen) throw new Error('Closed terminal.')
      const text = terminalRead ? '' : `${state.terminalWrites[0]}\n${failure === 'echo-only' ? '' : terminalResponse + '\r\n'}`
      terminalRead = true
      return { bytes: [...Buffer.from(text)] }
    }
    if (command === 'chat_answer_permission') {
      const entry = state.entries.find(entry => entry.runId === data.runId)
      assert.equal(entry.phase, 'pending-permission')
      assert.equal(entry.pendingPermission.gateId, data.gateId)
      assert.deepEqual(JSON.parse(JSON.stringify(data.answer)), { type: 'select', value: 'Allow once' })
      state.answers = [...(state.answers ?? []), data]
      entry.phase = 'complete'
      delete entry.pendingPermission
      fault('permission-answered', entry, state)
      return
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
  const invoke = async (command, data) => {
    const result = { value: await execute(command, data) }
    fault(`result:${command}`, result, state)
    return result.value
  }
  const context = { window: { __munimentSubscriptionTerminal: async options => new xterm.Terminal(options) }, document, URL, Uint8Array }
  vm.runInNewContext(fs.readFileSync('test/e2e/support/subscription-features.js', 'utf8'), context)
  const wait = async predicate => {
    for (let attempt = 0; attempt < 10; attempt++) {
      const result = await predicate()
      if (result) return result
    }
    throw Object.assign(new Error('The check timed out.'), { errorClass: 'timeout' })
  }
  const run = phase => context.window.__munimentSubscriptionFeatures({ plan: { ...plan, phase }, invoke, wait,
    setValue: (_element, value) => { fault('composer-input', undefined, state); prompt = value }, turns: plan.turns })
  const initial = { ...await run('chat'), ...await run('features') }
  const restart = await run('restart')
  await assert.rejects(run('update'))
  const updated = failure === 'subscription_probe_update' ? { 'signed-update': [] } : await run('update-restart')
  return { initial: JSON.parse(JSON.stringify({ ...initial, 'signed-update': updated['signed-update'] })),
    restart: JSON.parse(JSON.stringify(restart)), state }
}

test('the installed feature probe exercises commands and validates their results', async () => {
  let serverId
  const result = await probe(undefined, (command, data) => {
    if (command === 'extend_command' && data.action === 'server') serverId = data.data.id
  })
  for (const [feature, checks] of Object.entries(featureChecks)) {
    if (feature === 'local-startup') continue
    assert.deepEqual({ ...result.initial, ...result.restart }[feature], feature === 'signed-update' ? checks.slice(0, -1) : checks, feature)
  }
  assert.equal(result.state.content, `${fileNonce}\nEdited\n`)
  assert.equal(result.state.profile, '')
  assert.equal(result.state.projects.project, 'Renamed acceptance project')
  assert.equal(result.state.entries.length, 3)
  assert.equal(result.state.mcpSelected, true)
  assert.equal(serverId, 'release-acceptance')
  assert.deepEqual(JSON.parse(result.state.prompts[1].match(/\{.*\}/)[0]), {
    server: 'extend-release-acceptance', tool: 'acceptance_token', args: {},
  })
  assert.equal(result.state.prompts[1].includes(mcpNonce), false)
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
  const observed = { ...result.initial, ...result.restart }[feature]
  if (failure === 'subscription_probe_update') assert.deepEqual(observed, [])
  else {
    assert.equal(observed[0], 'failed')
    assert.ok(featureFailure(feature, observed).failure_stage)
  }
  assert.equal(JSON.stringify(result.initial).includes('private provider error'), false)
  if (feature !== 'projects') assert.deepEqual(result.initial.projects, featureChecks.projects)
})

for (const [feature, commands] of Object.entries({
  'account-balancing': ['model_router_settings'],
  routing: ['model_router_settings', 'model_router_save_routes', 'model_router_test_route', 'model_router_update_account'],
  memory: ['memory_profile_read', 'memory_profile_save'],
  terminal: ['terminal_start', 'terminal_write', 'terminal_read', 'terminal_close'],
  tools: ['chat_thread_open'],
})) {
  for (const command of commands) test(`the ${feature} probe names a rejected ${command} command`, async () => {
    for (const error of [undefined, null, 'C:\\private\\provider.txt', new Error('private provider text'),
      { errorClass: 'command-timeout' }, { errorClass: 'profile-restored' }]) {
      const result = await probe(undefined, name => { if (name === command) throw error })
      const reason = command === 'memory_profile_save'
        ? error?.errorClass === 'command-timeout' ? 'memory-save-timeout' : 'memory-save-rejected' : command
      const expected = feature === 'terminal' ? terminalFailure(reason,
        command === 'terminal_close' ? `${terminalCommand}\n${terminalResponse}\r\n` : '') : ['failed', 'check', reason]
      assert.deepEqual(result.initial[feature], expected)
      const { terminal_diagnostics, ...failure } = featureFailure(feature, result.initial[feature])
      assert.deepEqual(failure, { failure_stage: 'check', error_class: reason })
      if (feature === 'terminal') assert.equal(terminal_diagnostics.byte_count,
        command === 'terminal_close' ? Buffer.byteLength(`${terminalCommand}\n${terminalResponse}\r\n`) : 0)
      assert.equal(JSON.stringify(result.initial).includes('private'), false)
      assert.deepEqual(featureFailure(feature, ['failed', 'check', `${command}: C:\\private\\provider.txt`]), {})
      assert.deepEqual(featureFailure('files', ['failed', 'check', command]), {})
      assert.deepEqual(featureFailure(feature, ['failed', 'restore', command]), {})
    }
  })
}

for (const [failure, reason] of [
  ['single-account', 'multiple-accounts-served'],
  ['unequal-shares', 'equal-weight-shares'],
  ['active-reservation', 'no-active-reservations'],
]) test(`The account-balancing probe names ${failure}.`, async () => {
  const result = await probe(failure)
  assert.deepEqual(result.initial['account-balancing'], ['failed', 'check', reason])
  assert.deepEqual(featureFailure('account-balancing', result.initial['account-balancing']), {
    failure_stage: 'check', error_class: reason,
  })
})

for (const [name, change, reason = 'account-counters'] of [
  ['empty accounts', state => { state.accounts = [] }],
  ['missing accounts', state => { state.accounts = null }],
  ['null account', state => { state.accounts[0] = null }],
  ['duplicate IDs', state => { state.accounts[1].id = state.accounts[0].id }],
  ['empty ID', state => { state.accounts[0].id = '' }],
  ['missing family', state => { delete state.accounts[0].family }],
  ['invalid enabled flag', state => { state.accounts[0].enabled = 'true' }],
  ['unequal weights', state => { state.accounts[0].weight = 2 }, 'equal-account-weights'],
  ['provider errors', state => { state.accounts[0].errors = 1 }, 'account-errors'],
  ['different families', state => { state.accounts[1].family = 'anthropic' }, 'multiple-accounts-served'],
  ['unused accounts', state => { state.accounts.forEach(account => { account.requests = 0 }) }, 'multiple-accounts-served'],
]) test(`The account-balancing probe rejects ${name}.`, async () => {
  const result = await probe(undefined, (hook, _data, state) => { if (hook === 'setup') change(state) })
  const observed = result.initial['account-balancing']
  assert.deepEqual(observed, ['failed', 'check', reason])
  assert.deepEqual(featureFailure('account-balancing', observed), { failure_stage: 'check', error_class: reason })
  assert.deepEqual(featureFailure('files', observed), {})
  assert.deepEqual(featureFailure('account-balancing', ['failed', 'restore', reason]), {})
  assert.deepEqual(featureFailure('account-balancing', ['failed', 'check', `${reason}: private account data`]), {})
})

for (const field of ['requests', 'active', 'errors', 'weight']) {
  for (const value of [undefined, null, -1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, '2']) {
    test(`The account-balancing probe rejects ${field}=${String(value)}.`, async () => {
      const result = await probe(undefined, (hook, _data, state) => {
        if (hook === 'setup') state.accounts[0][field] = value
      })
      assert.deepEqual(result.initial['account-balancing'], ['failed', 'check', 'account-counters'])
    })
  }
}

test('The account-balancing probe waits for active reservations to end.', async () => {
  let reads = 0
  const result = await probe(undefined, (hook, data) => {
    if (hook === 'result:model_router_settings' && ++reads === 1) data.value.accounts[0].active = 1
  })
  assert.deepEqual(result.initial['account-balancing'], featureChecks['account-balancing'])
  assert.ok(reads > 1)
})

for (const [feature, reason, hook, change] of [
  ['routing', 'routing-accounts', 'result:model_router_settings', data => { data.value.accounts = null }],
  ['routing', 'fallback-selected', 'result:model_router_test_route', data => { data.value = null }],
  ['routing', 'empty-sample-rejected', 'model_router_test_route', data => { if (data.sample === '') data.sample = 'accepted' }],
  ['routing', 'invalid-fallback-rejected', 'model_router_save_routes', data => {
    if (data.fallback?.includes('nonexistent')) data.fallback = 'openai/model'
  }],
  ['routing', 'unavailable-model-excluded', 'result:model_router_test_route', data => {
    if (data.value.model === 'openai/model') data.value.eligible_models.push('openai/route-model')
  }],
  ['memory', 'profile-saved', 'result:memory_profile_read', (data, state) => {
    if (state.profile.includes('Acceptance profile.')) data.value = ''
  }],
  ['memory', 'profile-restored', 'memory_profile_save', data => { if (data.content === '') data.content = 'not restored' }],
  ['tools', 'composer-visible', 'query-selector', data => { data.value = null }],
  ['tools', 'composer-input', 'composer-input', () => { throw 'private provider text' }],
  ['tools', 'send-ready', 'query-buttons', () => { throw 'private provider text' }],
  ['tools', 'send-click', 'send-click', () => { throw 'private provider text' }],
]) test(`the ${feature} probe names the ${reason} assertion`, async () => {
  const result = await probe(undefined, (name, data, state) => { if (name === hook) change(data, state) })
  assert.deepEqual(result.initial[feature], ['failed', 'check', reason])
  assert.deepEqual(featureFailure(feature, result.initial[feature]), { failure_stage: 'check', error_class: reason })
  assert.equal(JSON.stringify(result.initial).includes('private'), false)
  assert.deepEqual(featureFailure('files', result.initial[feature]), {})
  assert.deepEqual(featureFailure(feature, ['failed', 'check', `${reason}: private provider text`]), {})
})

for (const [failure, reason] of [['echo-only', 'shell-output'], ['unclosed-terminal', 'shell-closed']]) {
  test(`the terminal probe names the ${reason} assertion`, async () => {
    const result = await probe(failure)
    const text = `${terminalCommand}\n${failure === 'echo-only' ? '' : terminalResponse + '\r\n'}`
    assert.deepEqual(result.initial.terminal, terminalFailure(reason, text))
    assert.deepEqual(featureFailure('terminal', result.initial.terminal), {
      failure_stage: 'check', error_class: reason,
      terminal_diagnostics: { byte_count: Buffer.byteLength(text), received: true, nonce_present: true, vt_present: false },
    })
    assert.ok(result.state.commands.includes('terminal_close'))
  })
}

for (const [name, text, passed] of [
  ['VT colors and erase sequences', `> ${terminalCommand}\n\x1b[32m${terminalResponse}\x1b[0m\x1b[K\r\n> `, true],
  ['ConPTY cursor rows without newlines', `\x1b[2J\x1b[H> ${terminalCommand.trimEnd()}\x1b[2;1H${terminalResponse}\x1b[K\x1b[3;1H> `, true],
  ['a Unicode title', `\x1b]0;终端\x07> ${terminalCommand}\n${terminalResponse}\r\n> `, true],
  ['an echoed command with VT sequences', `\x1b[H> echo \x1b[32m${nonce}\\-terminal\x1b[0m\r\n> `, false],
  ['a wrapped echoed command', `${'p'.repeat(75)}${terminalCommand}\n> `, false],
  ['a cursor-positioned echoed command', `${'p'.repeat(75)}echo \x1b[2;1H${nonce}\x1b[3;1H> `, false],
  ['a nonce in a title', `\x1b]0;${terminalResponse}\x1b\\> `, false],
  ['an output suffix', `${terminalResponse}unexpected\r\n> `, false],
  ['an overwritten row', `${terminalResponse}\rwrong\x1b[K\r\n> `, false],
  ['an unfinished row', terminalResponse, false],
  ['no bytes', '', false],
  ['private output without a nonce', 'C:\\private\\secret.txt\r\n> ', false],
]) test(`The terminal probe checks ${name}.`, async () => {
  const result = await probe(undefined, (hook, data, state) => {
    if (hook === 'result:terminal_read') {
      data.value.bytes = [...Buffer.from(state.outputRead ? '' : text)]
      state.outputRead = true
    }
  })
  assert.deepEqual(result.initial.terminal, passed ? featureChecks.terminal : terminalFailure('shell-output', text))
  assert.ok(result.state.commands.includes('terminal_close'))
  assert.equal(JSON.stringify(result.initial).includes('private'), false)
})

for (const platform of ['linux', 'macos-arm64', 'macos-x64', 'windows']) {
  for (const layout of ['plain', 'wrapped', 'cursor-positioned']) {
    for (const output of [false, true]) test(`The ${platform} terminal probe checks ${layout} command echo with output=${output}.`, async () => {
      let text
      const result = await probe(undefined, (hook, data, state) => {
        if (hook === 'setup') data.platform = platform
        if (hook === 'terminal_write') {
          const escape = platform === 'windows' ? '^' : '\\'
          assert.equal(data.data, `echo ${nonce}${escape}-terminal\r`)
          assert.equal(data.data.includes(terminalResponse), false)
          const command = data.data.trimEnd()
          const echo = layout === 'plain' ? `> ${command}\r\n`
            : layout === 'wrapped' ? `${'p'.repeat(75)}${command}\r\n`
            : `${'p'.repeat(75)}echo \x1b[2;1H${command.slice(5)}\x1b[3;1H`
          text = `${echo}${output ? `${terminalResponse}\x1b[4;1H` : ''}> `
        }
        if (hook === 'result:terminal_read') {
          data.value.bytes = [...Buffer.from(state.outputRead ? '' : text)]
          state.outputRead = true
        }
      })
      assert.deepEqual(result.initial.terminal, output ? featureChecks.terminal : terminalFailure('shell-output', text))
      assert.ok(result.state.commands.includes('terminal_close'))
    })
  }
}

test('The terminal command produces the response in the native shell.', async () => {
  const windows = process.platform === 'win32'
  const result = await probe(undefined, (hook, data) => {
    if (hook === 'setup') data.platform = windows ? 'windows' : 'linux'
  })
  const command = result.state.terminalWrites[0]
  const shell = spawnSync(windows ? 'cmd.exe' : '/bin/sh',
    [...(windows ? ['/d', '/s', '/c'] : ['-c']), command.trimEnd()], { encoding: 'utf8' })
  assert.equal(shell.status, 0, shell.stderr)
  assert.equal(shell.stdout, `${terminalResponse}${windows ? '\r\n' : '\n'}`)
  assert.equal(command.includes(terminalResponse), false)
})

test('The terminal probe parses VT sequences and the nonce across every byte boundary.', async () => {
  const bytes = Buffer.from(`\x1b]0;终端\x1b\\> ${terminalCommand.trimEnd()}\x1b[2;1H${terminalResponse}\x1b[K\x1b[3;1H> `)
  for (let split = 1; split < bytes.length; split++) {
    const chunks = [[], [...bytes.subarray(0, split)], [], [...bytes.subarray(split)]]
    const result = await probe(undefined, (hook, data) => {
      if (hook === 'result:terminal_read') data.value.bytes = chunks.shift() ?? []
    })
    assert.deepEqual(result.initial.terminal, featureChecks.terminal, `The split at byte ${split} must pass.`)
  }
})

test('The terminal probe answers a cursor query before it checks the output.', async () => {
  const writes = []
  let reads = 0
  const result = await probe(undefined, (hook, data) => {
    if (hook === 'terminal_write') writes.push(data.data)
    if (hook === 'result:terminal_read') {
      const text = ++reads === 1 ? '\x1b[6n' : reads === 2 && writes.includes('\x1b[1;1R') ? `${terminalResponse}\r\n> ` : ''
      data.value.bytes = [...Buffer.from(text)]
    }
  })
  assert.deepEqual(writes, [terminalCommand, '\x1b[1;1R'])
  assert.deepEqual(result.initial.terminal, featureChecks.terminal)
})

test('The terminal probe rejects a nonce prefix before a later suffix arrives.', async () => {
  const chunks = [terminalResponse, 'suffix\r\n> ']
  const result = await probe(undefined, (hook, data) => {
    if (hook === 'result:terminal_read') data.value.bytes = [...Buffer.from(chunks.shift() ?? '')]
  })
  assert.deepEqual(result.initial.terminal, terminalFailure('shell-output', `${terminalResponse}suffix\r\n> `))
})

test('The terminal probe records a split nonce without exporting output after a read failure.', async () => {
  const chunks = [`\x1b[H> echo ${nonce.slice(0, 15)}`, nonce.slice(15)]
  let reads = 0
  const result = await probe(undefined, (hook, data) => {
    if (hook === 'terminal_read' && ++reads === 3) throw new Error('private read error')
    if (hook === 'result:terminal_read') data.value.bytes = [...Buffer.from(chunks.shift() ?? '')]
    if (hook === 'terminal_close') throw new Error('private close error')
  })
  assert.deepEqual(result.initial.terminal, terminalFailure('terminal_read', `\x1b[H> echo ${nonce}`))
  assert.equal(JSON.stringify(result.initial).includes('private'), false)
})

test('the probes keep the first failure when cleanup also fails', async () => {
  const result = await probe('echo-only', (name, data, state) => {
    if (name === 'result:model_router_test_route') data.value.model = 'wrong'
    if (name === 'model_router_update_account' && data.models) throw 'private cleanup error'
    if (name === 'result:memory_profile_read' && state.profile) data.value = ''
    if (name === 'memory_profile_save' && data.content === '') throw 'private cleanup error'
    if (name === 'terminal_close') throw 'private cleanup error'
  })
  assert.deepEqual(result.initial.routing, ['failed', 'check', 'fallback-selected'])
  assert.deepEqual(result.initial.memory, ['failed', 'check', 'profile-saved'])
  assert.deepEqual(result.initial.terminal, terminalFailure('shell-output', `${terminalCommand}\n`))
  assert.equal(JSON.stringify(result.initial).includes('private'), false)
})

for (const [message, reason] of [
  ['Memory is unavailable.', 'memory-save-unavailable'],
  ['Memory is busy.', 'memory-save-busy'],
  ['The Home folder is unavailable.', 'memory-save-home'],
  ['The saved Home location could not be read.', 'memory-save-home'],
  ['The saved Home location is invalid.', 'memory-save-home'],
  ['The memory path is invalid.', 'memory-save-path'],
  ['The memory path must not be a symbolic link.', 'memory-save-path'],
  ['The memory folder cannot be created.', 'memory-save-folder'],
  ['The memory file could not be saved.', 'memory-save-write'],
  ['Keep the memory file under 64 KB.', 'memory-save-size'],
  ['Remove credentials before saving this memory.', 'memory-save-secret'],
  ['The memory file could not be saved. C:\\private\\profile.md', 'memory-save-rejected'],
  ['toString', 'memory-save-rejected'],
]) test(`The memory probe records ${reason} without rejection text.`, async () => {
  for (const error of [message, new Error(message), { errorClass: 'command-failed', cause: message }]) {
    const result = await probe(undefined, (name, data) => {
      if (name === 'memory_profile_save' && data.content !== '') throw error
    })
    assert.deepEqual(result.initial.memory, ['failed', 'check', reason])
    assert.deepEqual(featureFailure('memory', result.initial.memory), { failure_stage: 'check', error_class: reason })
    assert.deepEqual(featureFailure('files', result.initial.memory), {})
    assert.deepEqual(featureFailure('memory', ['failed', 'restore', reason]), {})
    assert.deepEqual(featureFailure('memory', ['failed', 'check', `${reason}: private content`]), {})
    assert.equal(JSON.stringify(result.initial).includes(message), false)
    assert.equal(result.state.profile, '')
  }
})

test('The memory probe keeps the save sub-code when restoration also rejects.', async () => {
  const result = await probe(undefined, (name, data) => {
    if (name === 'memory_profile_save') throw data.content === '' ? 'Memory is busy.' : 'The memory file could not be saved.'
  })
  assert.deepEqual(result.initial.memory, ['failed', 'check', 'memory-save-write'])
})

test('The memory probe reports a rejected restoration after a successful save.', async () => {
  const result = await probe(undefined, (name, data) => {
    if (name === 'memory_profile_save' && data.content === '') throw 'Memory is busy.'
  })
  assert.deepEqual(result.initial.memory, ['failed', 'check', 'memory-save-busy'])
  assert.equal(result.state.profile, '# Profile\n\nAcceptance profile.\n')
})

test('The installed invoke wrapper keeps the memory rejection for fixed-code classification.', async () => {
  const rejection = 'The memory file could not be saved.'
  let observed, caught
  const context = { window: {
    __MUNIMENT_SUBSCRIPTION_PLAN__: { phase: 'chat', models: [], acceptance: true },
    __TAURI__: { core: { invoke: async (command, data) => {
      if (command === 'memory_profile_save') throw rejection
      if (command === 'attach_listener_status') return { supervisor_running: true, connected: true }
      if (command === 'subscription_probe_observed') observed = data
    } } },
    __munimentSubscriptionFeatures: async ({ invoke }) => {
      try { await invoke('memory_profile_save', { content: '' }) } catch (error) { caught = error }
      return {}
    },
  }, document: {
    querySelector: () => ({ getClientRects: () => [1] }), querySelectorAll: () => [],
    createElement: () => ({}), head: { append() {} },
  }, setTimeout, clearTimeout }
  await vm.runInNewContext(fs.readFileSync('test/e2e/support/subscription-probe.js', 'utf8'), context)
  assert.equal(caught.errorClass, 'command-failed')
  assert.equal(caught.cause, rejection)
  assert.equal(observed.passed, true)
  assert.equal(JSON.stringify(observed).includes(rejection), false)
  const result = await probe(undefined, name => { if (name === 'memory_profile_save') throw caught })
  assert.deepEqual(result.initial.memory, ['failed', 'check', 'memory-save-write'])
})

test('the memory probe restores the profile after a failed save assertion', async () => {
  const result = await probe(undefined, (name, data, state) => {
    if (name === 'result:memory_profile_read' && state.profile) data.value = null
  })
  assert.deepEqual(result.initial.memory, ['failed', 'check', 'profile-saved'])
  assert.equal(result.state.profile, '')
})

test('the memory probe requires an exact restore, including Windows line endings', async () => {
  for (const normalize of [false, true]) {
    const profile = '# Profile\r\n\r\nOriginal profile.\r\n'
    const result = await probe(undefined, (name, data, state) => {
      if (name === 'setup') state.profile = profile
      if (normalize && name === 'memory_profile_save') data.content = data.content.replace(/\r\n/g, '\n')
    })
    assert.deepEqual(result.initial.memory, normalize ? ['failed', 'check', 'profile-restored'] : featureChecks.memory)
    if (!normalize) assert.equal(result.state.profile, profile)
  }
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
    assert.deepEqual(result.initial.browser, ['failed', 'check', failure === 'loading-forever' ? 'timeout' : 'check-failed'])
    assert.equal(result.state.snapshots, attempts)
    assert.equal(result.state.commands.filter(command => command === 'browser_view').length, 2)
  })
}

for (const [feature, stage, command, action] of [
  ['mcp', 'server-add', 'extend_command', 'server'],
  ['mcp', 'connection-test', 'extend_command', 'test'],
  ['mcp', 'toggle', 'mcp-toggle'], ['mcp', 'tool-turn', 'mcp-tool-turn'],
  ['mcp', 'server-remove', 'extend_command', 'remove'],
  ['artifacts', 'folders', 'workspace_folders'], ['artifacts', 'new-file', 'workspace_file_action'],
  ['artifacts', 'save', 'workspace_save_text'], ['artifacts', 'publish', 'artifact_from_file'],
  ['artifacts', 'read', 'artifact_read'], ['artifacts', 'rename', 'artifact_edit'],
  ['artifacts', 'rename', 'artifact_list'],
]) test(`the installed probe redacts the ${feature} ${stage} failure`, async () => {
  const result = await probe(undefined, (name, data) => {
    if (name === command && (!action || data.action === action)) {
      throw Object.assign(new Error('private provider error'), { errorClass: 'private provider error' })
    }
  })
  assert.deepEqual(result.initial[feature], ['failed', stage, 'check-failed'])
  assert.equal(JSON.stringify(result.initial).includes('private provider error'), false)
})

for (const feature of ['tools', 'mcp']) {
  test(`the ${feature} tool turn reports fixed reply sub-reasons without provider text`, async () => {
    const name = feature === 'tools' ? 'read' : 'mcp__acceptance_token'
    const cases = [
      ...['failed', 'cancelled', 'pending-permission'].map(phase => [{ phase }, `reply-phase-${phase}`]),
      [{ phase: 'interrupted' }, 'reply-interrupted-missing-reason-unapproved'],
      [{ phase: 'private provider phase' }, feature === 'tools' ? 'reply-complete' : 'timeout'],
      [{ text: 'private provider text' }, 'reply-text'], [{ text: '' }, 'reply-text'], [{ text: null }, 'reply-text'],
      [{ receipt: undefined }, 'receipt-tool'], [{ receipt: { tools: [] } }, 'receipt-tool'],
      [{ receipt: { tools: {} } }, 'receipt-tool'],
      ...[null, { name: 'private provider tool', calls: 1, failed: 0 },
        ...[0, -1, 0.5, '1', Infinity].map(calls => ({ name, calls, failed: 0 })),
        { name, calls: 1, failed: 1 }, { name, calls: 1, failed: '0' },
      ].map(item => [{ receipt: { tools: [item] } }, 'receipt-tool']),
    ]
    for (const [change, reason] of cases) {
      const result = await probe(undefined, (name, data) => {
        if (name === `${feature}-reply`) Object.assign(data, change)
        if (feature === 'mcp' && name === 'extend_command' && data.action === 'remove') {
          throw new Error('private cleanup error')
        }
      })
      const stage = feature === 'mcp' ? 'tool-turn' : 'check'
      assert.deepEqual(result.initial[feature], ['failed', stage, reason])
      assert.deepEqual(featureFailure(feature, result.initial[feature]), { failure_stage: stage, error_class: reason })
      assert.equal(JSON.stringify(result.initial).includes('private'), false)
    }
  })
}

test('the MCP probe answers its approval through the native one-shot path', async () => {
  let staleReads = 0
  const result = await probe(undefined, (command, data, state) => {
    if (command === 'mcp-reply') Object.assign(data, { phase: 'pending-permission', pendingPermission: structuredClone(mcpApproval) })
    if (command === 'permission-answered') Object.assign(data, { phase: 'pending-permission', pendingPermission: structuredClone(mcpApproval) })
    if (command === 'chat_thread_open' && state.answers?.length) {
      if (++staleReads === 1) state.entries.unshift({ runId: 'other-run', phase: 'pending-permission', pendingPermission: mcpApproval })
      if (staleReads === 3) state.entries.at(-1).phase = 'complete'
    }
  })
  assert.deepEqual(result.initial.mcp, featureChecks.mcp)
  assert.equal(result.state.answers.length, 1)
  assert.equal(result.state.answers[0].runId, result.state.entries.at(-1).runId)
  assert.equal(result.state.answers[0].gateId, mcpApproval.gateId)
})

test('the MCP probe rejects unrelated or malformed approval requests', async () => {
  for (const gate of [undefined, null, {},
    ...[{ kind: 'confirm' }, { gateId: '' }, { gateId: 42 }, { options: undefined }, { options: 'Allow once' },
      { options: ['Allow for session', 'Allow server for this session'] },
      { title: mcpApproval.title.replace('extend-release-acceptance', 'other-server') },
      { title: mcpApproval.title.replace('acceptance_token', 'other-tool') },
      { title: mcpApproval.title.replace('{}', '{"write":true}') },
      { title: `${mcpApproval.title}\nprivate provider text` },
    ].map(change => ({ ...mcpApproval, ...change })),
  ]) {
    const result = await probe(undefined, (command, data) => {
      if (command === 'mcp-reply') Object.assign(data, { phase: 'pending-permission', pendingPermission: gate })
    })
    assert.deepEqual(result.initial.mcp, ['failed', 'tool-turn', 'reply-phase-pending-permission'])
    assert.equal(result.state.commands.includes('chat_answer_permission'), false)
    assert.equal(JSON.stringify(result.initial).includes('private'), false)
  }
})

for (const mode of ['answer-rejected', 'still-pending', 'new-gate', 'failed', 'cancelled', 'interrupted', 'no-receipt', 'wrong-token']) {
  test(`the MCP approval does not pass a ${mode} turn`, async () => {
    const result = await probe(undefined, (command, data) => {
      if (command === 'mcp-reply') Object.assign(data, { phase: 'pending-permission', pendingPermission: structuredClone(mcpApproval) })
      if (command === 'chat_answer_permission' && mode === 'answer-rejected') throw new Error('private provider text')
      if (command === 'permission-answered') {
        if (['still-pending', 'new-gate'].includes(mode)) Object.assign(data, { phase: 'pending-permission',
          pendingPermission: { ...mcpApproval, gateId: mode === 'new-gate' ? 'new-gate' : mcpApproval.gateId } })
        if (['failed', 'cancelled', 'interrupted'].includes(mode)) data.phase = mode
        if (mode === 'no-receipt') delete data.receipt
        if (mode === 'wrong-token') data.text = 'private provider text'
      }
    })
    const reason = mode === 'no-receipt' ? 'receipt-tool' : mode === 'wrong-token' ? 'reply-text'
      : mode === 'interrupted' ? 'reply-interrupted-missing-reason-approved'
        : `reply-phase-${['failed', 'cancelled'].includes(mode) ? mode : 'pending-permission'}`
    assert.deepEqual(result.initial.mcp, ['failed', 'tool-turn', reason])
    assert.equal(result.state.commands.filter(command => command === 'chat_answer_permission').length, 1)
    assert.equal(JSON.stringify(result.initial).includes('private'), false)
  })
}

test('the MCP probe reports journal reasons and the approval state without provider text', async () => {
  for (const approved of [false, true]) {
    for (const [failureReason, reason] of [
      ['unknown-effect-outcome', 'unknown-effect-outcome'], ['interrupted', 'interrupted'],
      ['unspecified', 'unspecified'], ['private provider text', 'recorded'],
      [undefined, 'missing-reason'], [null, 'missing-reason'],
    ]) {
      const result = await probe(undefined, (command, data) => {
        if (command === 'mcp-reply') Object.assign(data, approved
          ? { phase: 'pending-permission', pendingPermission: structuredClone(mcpApproval) }
          : { phase: 'interrupted', failureReason })
        if (command === 'permission-answered') Object.assign(data, { phase: 'interrupted', failureReason })
      })
      const code = `reply-interrupted-${reason}-${approved ? 'approved' : 'unapproved'}`
      assert.deepEqual(result.initial.mcp, ['failed', 'tool-turn', code])
      assert.deepEqual(featureFailure('mcp', result.initial.mcp), { failure_stage: 'tool-turn', error_class: code })
      assert.equal(result.state.answers?.length ?? 0, approved ? 1 : 0)
      assert.equal(JSON.stringify(result.initial).includes('private'), false)
    }
  }
})

test('the tools probe never answers an MCP approval', async () => {
  const result = await probe(undefined, (command, data) => {
    if (command === 'tools-reply') Object.assign(data, { phase: 'pending-permission', pendingPermission: mcpApproval })
  })
  assert.deepEqual(result.initial.tools, ['failed', 'check', 'reply-phase-pending-permission'])
  assert.equal(result.state.commands.includes('chat_answer_permission'), false)
})

test('the MCP probe rejects provider errors that claim a local reply sub-reason', async () => {
  for (const errorClass of ['reply-phase', 'reply-phase-failed', 'reply-phase-cancelled', 'reply-phase-interrupted',
    'reply-phase-pending-permission', 'reply-text', 'receipt-tool', ...interruptionErrors]) {
    const result = await probe(undefined, name => {
      if (name === 'mcp-tool-turn') throw Object.assign(new Error('private provider error'), { errorClass })
    })
    assert.deepEqual(result.initial.mcp, ['failed', 'tool-turn', 'check-failed'])
  }
})

test('the MCP probe keeps the first failure when server removal also fails', async () => {
  const result = await probe(undefined, (name, data) => {
    if (name === 'extend_command' && ['test', 'remove'].includes(data.action)) {
      throw Object.assign(new Error('private provider error'), { errorClass: data.action === 'test' ? 'command-timeout' : 'unknown' })
    }
  })
  assert.deepEqual(result.initial.mcp, ['failed', 'connection-test', 'timeout'])
})

test('the artifact probe reports an empty folder or file result without provider text', async () => {
  for (const failure of ['empty-folders', 'empty-files']) {
    const result = await probe(failure)
    assert.deepEqual(result.initial.artifacts, ['failed', failure === 'empty-folders' ? 'folders' : 'new-file', 'check-failed'])
  }
})

test('the main window can read published artifacts without granting access to browser views', () => {
  const capability = JSON.parse(fs.readFileSync('src-tauri/capabilities/default.json', 'utf8'))
  assert.deepEqual(capability.windows, ['main'])
  for (const permission of ['allow-artifact-from-file', 'allow-artifact-read', 'allow-artifact-edit', 'allow-artifact-list']) {
    assert.ok(capability.permissions.includes(permission), permission)
  }
})

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
        if (command === 'subscription_probe_progress') return
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
      setTimeout, clearTimeout,
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

test('the MCP receipt check preserves the first failure and rejects incomplete receipts', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-receipt-'))
  const file = path.join(root, 'receipt.json')
  try {
    for (const bytes of [undefined, '', '{', 'null', '{}', '{"token":"private-provider-text"}']) {
      if (bytes !== undefined) fs.writeFileSync(file, bytes)
      assert.deepEqual(verifyMcpReceipt(featureChecks.mcp, file, mcpNonce), ['failed', 'receipt', 'check-failed'])
      const failure = ['failed', 'connection-test', 'timeout']
      assert.deepEqual(verifyMcpReceipt(failure, file, mcpNonce), failure)
    }
    fs.writeFileSync(file, JSON.stringify({ token: mcpNonce }))
    assert.deepEqual(verifyMcpReceipt(featureChecks.mcp, file, mcpNonce), featureChecks.mcp)
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
})

test('the MCP bridge saves a new stdio server and preserves only matching icons', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-bridge-'))
  const definition = { command: process.execPath, args: ['fixture.mjs'] }
  const inventory = path.join(root, 'extensions', 'inventory.json')
  const call = data => {
    const result = spawnSync(process.execPath, [corePath('crates/core/src/extend_bridge.mjs')], {
      env: { ...process.env, MUNIMENT_EXTEND_ROOT: root }, input: JSON.stringify({ action: 'server', data }),
      encoding: 'utf8', timeout: 10_000,
    })
    assert.equal(result.status, 0, result.stdout + result.stderr)
    return JSON.parse(result.stdout.trim().slice('MUNIMENT_EXTEND_RESULT='.length)).result.items
  }
  try {
    const data = { id: 'release-acceptance', name: 'Release acceptance', definition }
    const items = call(data)
    assert.equal(items[0].icon, null)
    assert.equal(items[0].definition.command, process.execPath)
    items[0].icon = 'saved-icon'
    fs.writeFileSync(inventory, JSON.stringify({ items }))
    assert.equal(call({ ...data, name: 'Renamed acceptance' })[0].icon, 'saved-icon')
    assert.equal(call({ ...data, definition: { url: 'https://example.com/mcp' } })[0].icon, null)
    assert.equal(call({ name: 'Another stdio server', definition }).length, 2)
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
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
    const ca = fs.readFileSync(path.join(root, 'update-cert.pem'))
    const get = (url, options = {}) => new Promise((resolve, reject) => {
      https.get(url, { ca, ...options }, response => {
        const chunks = []
        response.on('data', chunk => chunks.push(chunk))
        response.on('end', () => resolve(Buffer.concat(chunks)))
      }).on('error', reject)
    })
    await assert.rejects(get(server.url, { ca: [] }), { code: 'DEPTH_ZERO_SELF_SIGNED_CERT' })
    await assert.rejects(get(server.url, { servername: 'localhost' }), { code: 'ERR_TLS_CERT_ALTNAME_INVALID' })
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

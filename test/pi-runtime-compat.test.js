// @vitest-environment node
import { afterEach, describe, expect, it } from 'vitest'
import { createServer } from 'node:http'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, execFileSync } from 'node:child_process'
import { readPins } from '../scripts/check-agent-dependencies.mjs'
import { corePath } from '../scripts/muniment-core.mjs'
import { writeAgentExtension } from './e2e/support/agent-extension.mjs'

// Run against an isolated frozen package install, never the user's profile.
const executable = process.env.PI_TEST_BINARY
const packages = process.env.PI_TEST_PACKAGES
const cleanups = []
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup() })

describe.skipIf(!executable || !packages)('shipped Pi and extension compatibility', () => {
  it('calls a user MCP server from a codemode script with Pi\'s own MCP support', async () => {
    const root = mkdtempSync(join(tmpdir(), 'muniment-mcp-test-'))
    cleanups.push(() => rmSync(root, { recursive: true, force: true }))
    const token = `MUNIMENT-${'c'.repeat(32)}`
    const receipt = join(root, 'receipt.json')
    writeFileSync(join(root, 'mcp.json'), JSON.stringify({ mcpServers: {
      'extend-release-acceptance': { command: process.execPath,
        args: [resolve('test/e2e/support/subscription-mcp.mjs'), token, receipt] },
    } }))
    const script = 'const result = await tools.mcp__extend_release_acceptance__acceptance_token({}); return result.content'
    let requests = 0
    let declared = []
    const server = createServer((request, response) => {
      let body = ''
      request.on('data', chunk => { body += chunk })
      request.on('end', () => {
        if (requests++ === 0) declared = JSON.parse(body).tools.map(tool => tool.function.name)
        response.writeHead(200, { 'content-type': 'text/event-stream' })
        const chunks = requests === 1 ? [
          { delta: { role: 'assistant', tool_calls: [{ index: 0, id: 'call-1', type: 'function',
            function: { name: 'codemode', arguments: JSON.stringify({ code: script }) } }] }, finish_reason: null },
          { delta: {}, finish_reason: 'tool_calls' },
        ] : [{ delta: { role: 'assistant', content: token }, finish_reason: null }, { delta: {}, finish_reason: 'stop' }]
        for (const chunk of chunks) response.write(`data: ${JSON.stringify({ id: 'fixture', object: 'chat.completion.chunk', model: 'fixture', choices: [{ index: 0, ...chunk }] })}\n\n`)
        response.end('data: [DONE]\n\n')
      })
    })
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
    cleanups.push(() => new Promise(resolve => server.close(resolve)))
    writeFileSync(join(root, 'models.json'), JSON.stringify({ providers: { fixture: {
      baseUrl: `http://127.0.0.1:${server.address().port}/v1`, api: 'openai-completions', apiKey: 'fixture',
      models: [{ id: 'fixture', contextWindow: 128000, maxTokens: 8192, input: ['text'] }],
    } } }))
    const result = await new Promise((resolveResult, reject) => {
      const child = spawn(executable, ['-p', '--mode', 'json', '--no-session', '--no-skills', '--no-context-files',
        '--provider', 'fixture', '--model', 'fixture', 'Call the MCP tool.'], {
        cwd: root, env: { PATH: process.env.PATH, HOME: root, TMPDIR: root, PI_CODING_AGENT_DIR: root, PI_OFFLINE: '1' },
        stdio: ['ignore', 'pipe', 'pipe'],
      })
      let stdout = '', stderr = ''
      const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('The MCP test timed out.')) }, 30000)
      child.stdout.on('data', data => { stdout += data })
      child.stderr.on('data', data => { stderr += data })
      child.on('error', error => { clearTimeout(timer); reject(error) })
      child.on('close', code => { clearTimeout(timer); resolveResult({ code, stdout, stderr }) })
    })
    expect(result.stderr).not.toMatch(/Failed to load extension|extension.*error/i)
    expect(result.code, result.stderr).toBe(0)
    // A server with codemode exposure turns codemode on and keeps its tools out of the declarations.
    expect(declared).toContain('codemode')
    expect(declared.some(name => name.startsWith('mcp__'))).toBe(false)
    const events = result.stdout.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line))
    const ends = events.filter(event => event.type === 'tool_execution_end')
    const nested = ends.find(event => event.toolName === 'mcp__extend_release_acceptance__acceptance_token')
    expect(nested?.parentToolCallId, JSON.stringify(ends)).toBe('call-1')
    expect(nested.result.content).toContainEqual({ type: 'text', text: token })
    expect(ends.find(event => event.toolName === 'codemode').isError).toBe(false)
    expect(JSON.parse(readFileSync(receipt, 'utf8'))).toEqual({ token })
    expect(requests).toBe(2)
  }, 40000)

  it('loads every extension and completes streamed replies with cache usage for four selected models', async () => {
    expect(execFileSync(executable, ['--version'], { encoding: 'utf8', timeout: 10000 }).trim()).toBe(readPins().pi)
    const root = mkdtempSync(join(tmpdir(), 'muniment-pi-test-'))
    cleanups.push(() => rmSync(root, { recursive: true, force: true }))
    const requested = []
    const server = createServer((request, response) => {
      if (request.method !== 'POST') {
        response.writeHead(200, { 'content-type': 'application/json' })
        response.end(JSON.stringify({ stages: [], done: true }))
        return
      }
      let body = ''
      request.on('data', chunk => { body += chunk })
      request.on('end', () => {
        const { model } = JSON.parse(body)
        requested.push(model)
        response.writeHead(200, { 'content-type': 'text/event-stream' })
        for (const chunk of [
          { choices: [{ index: 0, delta: { role: 'assistant', content: '12' }, finish_reason: null }] },
          { choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] },
          { choices: [], usage: { prompt_tokens: 100, completion_tokens: 2, total_tokens: 102, prompt_tokens_details: { cached_tokens: 50 }, completion_tokens_details: { reasoning_tokens: 0 } } },
        ]) response.write(`data: ${JSON.stringify({ id: 'fixture', object: 'chat.completion.chunk', model, ...chunk })}\n\n`)
        response.end('data: [DONE]\n\n')
      })
    })
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
    cleanups.push(() => new Promise(resolve => server.close(resolve)))
    const models = ['anthropic/claude-opus-5-5', 'openai/gpt-6-luna', 'xai/grok-4.7', 'openai/gpt-6-sol']
    writeFileSync(join(root, 'models.json'), JSON.stringify({ providers: { 'muniment-router': {
      baseUrl: `http://127.0.0.1:${server.address().port}/v1`, api: 'openai-completions', apiKey: 'fixture',
      models: models.map(id => ({ id, contextWindow: 128000, maxTokens: 8192, input: ['text'] })),
    } } }))
    const extensions = []
    for (const [name, version] of Object.entries(readPins().packages)) {
      const path = resolve(packages, 'node_modules', name)
      const manifest = JSON.parse(readFileSync(join(path, 'package.json'), 'utf8'))
      expect(manifest.version).toBe(version)
      for (const entry of manifest.pi.extensions) extensions.push('-e', resolve(path, entry))
    }
    extensions.push('-e', corePath('crates/core/src/assistant_identity.mjs'), '-e', corePath('crates/core/src/routing_progress.mjs'))
    for (const model of models) {
      const result = await new Promise((resolveResult, reject) => {
        const child = spawn(executable, ['-p', '--mode', 'json', '--no-session', '--no-extensions', '--no-skills', '--no-context-files', '--no-tools', ...extensions, '--provider', 'muniment-router', '--model', model, 'What is 6 + 6?'], {
          cwd: root, env: { PATH: process.env.PATH, HOME: root, TMPDIR: root, PI_CODING_AGENT_DIR: root, PI_OFFLINE: '1' },
          stdio: ['ignore', 'pipe', 'pipe'],
        })
        let stdout = '', stderr = ''
        const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('Pi compatibility test timed out')) }, 30000)
        child.stdout.on('data', data => { stdout += data })
        child.stderr.on('data', data => { stderr += data })
        child.on('error', error => { clearTimeout(timer); reject(error) })
        child.on('close', code => { clearTimeout(timer); resolveResult({ code, stdout, stderr }) })
      })
      expect(result.stderr).not.toMatch(/Failed to load extension|extension.*error|errors loading models/i)
      expect(result.code, result.stderr).toBe(0)
      const events = result.stdout.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line))
      const reply = events.find(event => event.type === 'message_end' && event.message?.role === 'assistant')?.message
      expect(reply, result.stdout).toBeDefined()
      expect(reply.stopReason, reply.errorMessage).toBe('stop')
      expect(reply.model).toBe(model)
      expect(reply.content).toContainEqual({ type: 'text', text: '12' })
      expect(reply.usage.cacheRead).toBe(50)
    }
    expect(requested).toEqual(models)
  }, 120000)

  it('answers find and grep without fd or ripgrep offline and normalizes question arguments', async () => {
    const root = mkdtempSync(join(tmpdir(), 'muniment-search-test-'))
    cleanups.push(() => rmSync(root, { recursive: true, force: true }))
    const extension = writeAgentExtension()
    cleanups.push(extension.cleanup)
    const work = join(root, 'work')
    mkdirSync(join(work, 'notes dir'), { recursive: true })
    writeFileSync(join(work, 'notes dir', 'plan.md'), 'first\nship the needle\n')
    writeFileSync(join(work, 'readme.txt'), 'no match here\n')
    // An empty PATH and a fresh agent directory leave Pi no fd or rg, and offline it cannot download them.
    const bin = join(root, 'empty-bin')
    mkdirSync(bin)
    const calls = [
      ['find', { pattern: '*.md' }],
      ['grep', { pattern: 'needle' }],
      ['ask_user_question', { question: 'Which format?', choices: ['PDF', 'CSV'] }],
      ['ask_user_question', { questions: [{ question: 'Which folder?' }] }],
      ['ask_user_question', { prompt: 'Which folder?' }],
    ]
    let requests = 0
    let declared = []
    const server = createServer((request, response) => {
      let body = ''
      request.on('data', chunk => { body += chunk })
      request.on('end', () => {
        if (requests++ === 0) declared = JSON.parse(body).tools.map(tool => tool.function)
        response.writeHead(200, { 'content-type': 'text/event-stream' })
        const chunks = requests === 1 ? [
          { delta: { role: 'assistant', tool_calls: calls.map(([name, args], index) => ({ index, id: `call-${index}`, type: 'function',
            function: { name, arguments: JSON.stringify(args) } })) }, finish_reason: null },
          { delta: {}, finish_reason: 'tool_calls' },
        ] : [{ delta: { role: 'assistant', content: 'done' }, finish_reason: null }, { delta: {}, finish_reason: 'stop' }]
        for (const chunk of chunks) response.write(`data: ${JSON.stringify({ id: 'fixture', object: 'chat.completion.chunk', model: 'fixture', choices: [{ index: 0, ...chunk }] })}\n\n`)
        response.end('data: [DONE]\n\n')
      })
    })
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
    cleanups.push(() => new Promise(resolve => server.close(resolve)))
    writeFileSync(join(root, 'models.json'), JSON.stringify({ providers: { fixture: {
      baseUrl: `http://127.0.0.1:${server.address().port}/v1`, api: 'openai-completions', apiKey: 'fixture',
      models: [{ id: 'fixture', contextWindow: 128000, maxTokens: 8192, input: ['text'] }],
    } } }))
    const result = await new Promise((resolveResult, reject) => {
      const child = spawn(executable, ['-p', '--mode', 'json', '--no-session', '--no-extensions', '--no-skills', '--no-context-files',
        '--tools', 'read,find,grep,ls,ask_user_question', '-e', extension.file,
        '--provider', 'fixture', '--model', 'fixture', 'Find the plan.'], {
        cwd: work, env: { PATH: bin, HOME: root, TMPDIR: root, PI_CODING_AGENT_DIR: root, PI_OFFLINE: '1' },
        stdio: ['ignore', 'pipe', 'pipe'],
      })
      let stdout = '', stderr = ''
      const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('The search test timed out.')) }, 30000)
      child.stdout.on('data', data => { stdout += data })
      child.stderr.on('data', data => { stderr += data })
      child.on('error', error => { clearTimeout(timer); reject(error) })
      child.on('close', code => { clearTimeout(timer); resolveResult({ code, stdout, stderr }) })
    })
    expect(result.stderr).not.toMatch(/Failed to load extension|extension.*error/i)
    expect(result.code, result.stderr).toBe(0)
    // The model sees the native schemas and the canonical question schema with its example.
    const schema = name => declared.find(tool => tool.name === name)
    expect(Object.keys(schema('grep').parameters.properties)).toEqual(['pattern', 'path', 'glob', 'ignoreCase', 'literal', 'context', 'limit'])
    expect(schema('ask_user_question').parameters.required).toEqual(['questions'])
    expect(schema('ask_user_question').description).toContain('Arguments example: {"questions"')
    const events = result.stdout.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line))
    const ends = Object.fromEntries(events.filter(event => event.type === 'tool_execution_end').map(event => [event.toolCallId, event]))
    const output = id => ends[id]?.result.content.map(part => part.text).join('')
    for (const id of ['call-0', 'call-1']) expect(output(id), JSON.stringify(ends[id])).not.toMatch(/not available|could not be downloaded/)
    expect(ends['call-0'].isError).toBe(false)
    expect(output('call-0')).toBe('notes dir/plan.md')
    expect(ends['call-1'].isError).toBe(false)
    expect(output('call-1')).toBe('notes dir/plan.md:2: ship the needle')
    // Print mode has no user to answer, so accepted questions end unanswered rather than invalid.
    for (const id of ['call-2', 'call-3']) {
      expect(ends[id].isError, output(id)).toBe(false)
      expect(ends[id].result.details).toEqual({ cancelled: true })
    }
    expect(ends['call-4'].isError).toBe(true)
    expect(output('call-4')).toContain('Example: {"questions"')
    expect(requests).toBe(2)
  }, 40000)
})

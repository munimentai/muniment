// @vitest-environment node
import { afterEach, describe, expect, it } from 'vitest'
import { createServer } from 'node:http'
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, execFileSync } from 'node:child_process'
import { readPins } from '../scripts/check-agent-dependencies.mjs'

// Run against an isolated frozen package install, never the user's profile.
const executable = process.env.PI_TEST_BINARY
const packages = process.env.PI_TEST_PACKAGES
const cleanups = []
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup() })

describe.skipIf(!executable || !packages)('shipped Pi and extension compatibility', () => {
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
    extensions.push('-e', resolve('src-tauri/core/src/assistant_identity.mjs'), '-e', resolve('src-tauri/core/src/routing_progress.mjs'))
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
})

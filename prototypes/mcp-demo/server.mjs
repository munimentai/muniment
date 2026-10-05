import { createServer } from 'node:http'
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { randomBytes } from 'node:crypto'
import { build } from 'esbuild'
import { createServer as createViteServer } from 'vite'
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js'
import { DemoBoundary } from './boundary.mjs'
import { createFixture } from './fixture.mjs'

const directory = dirname(fileURLToPath(import.meta.url))
async function listen(server) {
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve) })
  return `http://127.0.0.1:${server.address().port}`
}
async function closeServer(server) {
  server.closeAllConnections()
  await new Promise(resolve => server.close(resolve))
}
async function body(request) {
  let text = ''
  for await (const chunk of request) {
    text += chunk
    if (text.length > 64000) throw new Error('The request exceeds the demo limit.')
  }
  return JSON.parse(text)
}
function json(response, status, value) {
  response.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store', 'x-content-type-options': 'nosniff' })
  response.end(JSON.stringify(value))
}
export async function startDemo({ ui = true } = {}) {
  const assets = await mkdtemp(join(tmpdir(), 'muniment-mcp-assets-'))
  let fixtureServer, modelServer, boundary, vite, server
  const dispose = async () => {
    if (server) await closeServer(server)
    await boundary?.close()
    await vite?.close()
    if (modelServer) await closeServer(modelServer)
    if (fixtureServer) await closeServer(fixtureServer)
    await rm(assets, { recursive: true, force: true })
  }
  try {
    const bundle = await build({ entryPoints: [join(directory, 'fixture-app.js')], bundle: true, write: false, format: 'iife', platform: 'browser' })
    const template = await readFile(join(directory, 'fixture-app.html'), 'utf8')
    const html = template.replace('/* APP_SCRIPT */', () => bundle.outputFiles[0].text.replaceAll('</script', '<\\/script'))
    const htmlPath = join(assets, 'quote.html')
    await writeFile(htmlPath, html, { mode: 0o600 })
    fixtureServer = createServer(async (request, response) => {
      if (request.url !== '/mcp' || request.method !== 'POST') { response.writeHead(405); response.end(); return }
      const server = createFixture(html)
      const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined, enableJsonResponse: true })
      response.on('close', () => { void transport.close(); void server.close() })
      try { await server.connect(transport); await transport.handleRequest(request, response, await body(request)) }
      catch { if (!response.headersSent) json(response, 400, { error: 'The fixture request failed.' }); else response.end() }
    })
    const fixtureUrl = await listen(fixtureServer)
    modelServer = createServer(async (request, response) => {
      try {
        if (request.method !== 'POST' || request.url !== '/v1/chat/completions' || !boundary?.pendingModel) { json(response, 404, {}); return }
        const input = await body(request)
        const pending = boundary.pendingModel
        if (++pending.requests > 2) { json(response, 400, { error: 'The fixture accepts one tool call per chat.' }); return }
        const call = pending.requests === 1
        if (call && !input.tools?.some(tool => tool.function?.name === pending.tool)) { json(response, 400, { error: 'The candidate did not declare the requested tool.' }); return }
        response.writeHead(200, { 'content-type': 'text/event-stream' })
        const toolResult = input.messages.filter(message => message.role === 'tool').at(-1)?.content
        const reply = typeof toolResult === 'string' ? toolResult : JSON.stringify(toolResult ?? 'The tool returned no text.')
        const chunks = call ? [
          { delta: { role: 'assistant', tool_calls: [{ index: 0, id: 'demo-call', type: 'function', function: { name: pending.tool, arguments: JSON.stringify(pending.args) } }] }, finish_reason: null },
          { delta: {}, finish_reason: 'tool_calls' },
        ] : [{ delta: { role: 'assistant', content: reply }, finish_reason: null }, { delta: {}, finish_reason: 'stop' }]
        for (const chunk of chunks) response.write(`data: ${JSON.stringify({ id: 'demo', object: 'chat.completion.chunk', model: 'fixture', choices: [{ index: 0, ...chunk }] })}\n\n`)
        response.end('data: [DONE]\n\n')
      } catch { if (!response.headersSent) json(response, 400, { error: 'The model fixture request failed.' }); else response.end() }
    })
    const modelUrl = await listen(modelServer)
    const fixture = { stdio: { command: process.execPath, args: [join(directory, 'fixture.mjs'), htmlPath] }, http: { url: `${fixtureUrl}/mcp` } }
    boundary = await DemoBoundary.create(fixture, `${modelUrl}/v1`)
    const token = randomBytes(32).toString('hex')
    vite = ui ? await createViteServer({ configFile: join(directory, 'vite.config.js'), root: directory, server: { middlewareMode: true }, appType: 'custom' }) : null
    let origin
    server = createServer(async (request, response) => {
      if (request.headers.host !== new URL(origin).host || (request.headers.origin && request.headers.origin !== origin)) { json(response, 403, { error: 'Open the demo from its local launch URL.' }); return }
      if (request.url.startsWith('/api/')) {
        if (request.headers['x-demo-token'] !== token) { json(response, 403, { error: 'Reload the demo page before trying again.' }); return }
        try {
          if (request.url === '/api/info' && request.method === 'GET') { json(response, 200, { fixture, profile: boundary.root, version: '0.99.1' }); return }
          if (request.url !== '/api/command' || request.method !== 'POST') { json(response, 404, {}); return }
          const { action, data } = await body(request)
          const result = await boundary.dispatch(action, data)
          json(response, 200, { result })
        } catch (error) { json(response, 400, { error: error.message }) }
        return
      }
      if (request.url === '/' && vite) {
        let page = await readFile(join(directory, 'index.html'), 'utf8')
        page = page.replace('DEMO_TOKEN', token)
        response.writeHead(200, { 'content-type': 'text/html', 'cache-control': 'no-store', 'x-frame-options': 'DENY' })
        response.end(await vite.transformIndexHtml('/', page))
      } else if (vite) vite.middlewares(request, response, () => { response.writeHead(404); response.end() })
      else { response.writeHead(404); response.end() }
    })
    origin = await listen(server)
    return { origin, token, boundary, fixture, close: dispose }
  } catch (error) { await dispose(); throw error }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const demo = await startDemo()
  console.log(`Open ${demo.origin}/ to compare the candidate. Press Ctrl+C to delete the disposable profile.`)
  let closing = false
  const close = async () => { if (closing) return; closing = true; await demo.close(); process.exit(0) }
  process.on('SIGINT', close)
  process.on('SIGTERM', close)
}

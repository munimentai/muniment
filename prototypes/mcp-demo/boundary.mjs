import { mkdtemp, mkdir, writeFile, rename, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { randomUUID } from 'node:crypto'
import { isDeepStrictEqual } from 'node:util'
import { spawn } from 'node:child_process'
import { McpClient, StdioTransport, StreamableHttpTransport } from '@earendil-works/pi-mcp'

const directory = dirname(fileURLToPath(import.meta.url))
const piCli = fileURLToPath(new URL('./bundle/cli.js', import.meta.resolve('@earendil-works/pi-coding-agent')))
export const chatFlags = ['-p', '--mode', 'json', '--no-session', '--no-extensions', '--no-skills', '--no-prompt-templates', '--no-context-files',
  '-e', 'builtin:mcp', '-e', 'builtin:codemode', '-e', join(directory, 'permission.mjs'), '--provider', 'fixture', '--model', 'fixture']
const connectionError = 'Connection failed. Check the URL, command, and credentials, then test again.'
const reference = /^\$\{DEMO_[A-Z0-9_]+\}$/

export function validateDefinition(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Enter a server configuration.')
  const allowed = ['command', 'args', 'url', 'type', 'env', 'headers', 'oauth', 'auth', 'timeout', 'exposure']
  if (Object.keys(value).some(key => !allowed.includes(key))) throw new Error('This demo does not support that configuration field. Keep the original entry unchanged.')
  if (value.type !== undefined && !['stdio', 'http', 'streamable-http'].includes(value.type)) throw new Error('Use stdio or streamable HTTP. The candidate does not support SSE or Unix sockets.')
  if ((value.url !== undefined) === (value.command !== undefined)) throw new Error('Enter one URL or one command, not both.')
  if (value.url !== undefined) {
    if (typeof value.url !== 'string') throw new Error('Enter an HTTP or HTTPS server URL.')
    let url
    try { url = new URL(value.url) } catch { throw new Error('Enter an HTTP or HTTPS server URL.') }
    if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || url.hash) throw new Error('Use an HTTP or HTTPS URL without credentials or a fragment.')
    if (value.type === 'stdio' || value.args !== undefined || value.env !== undefined) throw new Error('Remove the stdio fields from the HTTP configuration.')
  } else {
    if (typeof value.command !== 'string' || !value.command.trim()) throw new Error('Enter a command.')
    if ((value.type !== undefined && value.type !== 'stdio') || value.headers !== undefined || value.oauth !== undefined || value.auth !== undefined) throw new Error('Remove the HTTP fields from the stdio configuration.')
    if (value.args !== undefined && (!Array.isArray(value.args) || value.args.some(arg => typeof arg !== 'string'))) throw new Error('Server arguments must be a list of text values.')
  }
  for (const key of ['headers', 'env']) {
    const values = value[key]
    if (values !== undefined && (!values || typeof values !== 'object' || Array.isArray(values))) throw new Error('Use an object for headers and environment variables.')
    for (const secret of Object.values(values || {})) if (typeof secret !== 'string' || !reference.test(secret)) throw new Error('Use DEMO_ environment references for secrets. The demo does not read your profile.')
  }
  if (value.auth !== undefined && value.auth !== 'oauth') throw new Error('Use OAuth or the bearer token field.')
  if (value.oauth !== undefined) throw new Error('This demo supports automatic OAuth registration only. Keep custom OAuth settings in your original profile.')
  if (value.timeout !== undefined && (!Number.isFinite(value.timeout) || value.timeout < 1 || value.timeout > 30)) throw new Error('Set the timeout between 1 and 30 seconds.')
  if (value.exposure !== undefined && !['direct', 'codemode'].includes(value.exposure)) throw new Error('Choose direct or codemode exposure for this comparison.')
  const { auth, ...definition } = value
  return { ...definition, timeout: definition.timeout ?? 10, exposure: definition.exposure ?? 'codemode' }
}

export function runPi(args, { cwd, env, timeout = 45000 }) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [piCli, ...args], { cwd, env, detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'] })
    const signal = name => {
      try { if (process.platform === 'win32') child.kill(name); else if (child.pid) process.kill(-child.pid, name) }
      catch (error) { if (error.code !== 'ESRCH') throw error }
    }
    let stdout = '', stderr = '', exceeded = false
    const stop = () => { exceeded = true; signal('SIGTERM'); kill = setTimeout(() => signal('SIGKILL'), 2000) }
    let kill
    const timer = setTimeout(stop, timeout)
    child.stdout.on('data', chunk => { if (stdout.length < 2_000_000) stdout += chunk; else if (!exceeded) stop() })
    child.stderr.on('data', chunk => { if (stderr.length < 100_000) stderr += chunk })
    child.on('error', error => { clearTimeout(timer); clearTimeout(kill); reject(error) })
    child.on('close', code => {
      clearTimeout(timer); clearTimeout(kill)
      if (exceeded) { signal('SIGKILL'); reject(new Error('The operation timed out. Check the server and retry.')) }
      else resolve({ code, stdout, stderr })
    })
  })
}

export class DemoBoundary {
  static async create(fixture, modelUrl) {
    const root = await mkdtemp(join(tmpdir(), 'muniment-mcp-profile-'))
    const boundary = new DemoBoundary(root, fixture, modelUrl)
    await mkdir(join(root, 'agent'), { mode: 0o700 })
    await mkdir(join(root, 'work'), { mode: 0o700 })
    await boundary.save()
    return boundary
  }
  constructor(root, fixture, modelUrl) {
    this.root = root
    this.fixture = fixture
    this.modelUrl = modelUrl
    this.state = { items: [], threads: {} }
    this.tokens = new Map()
    this.serverTools = new Map()
    this.tail = Promise.resolve()
  }
  dispatch(action, data = {}) {
    const result = this.tail.then(async () => {
      const before = ['server', 'toggle', 'remove'].includes(action)
        ? { state: structuredClone(this.state), tokens: new Map(this.tokens), tools: new Map(this.serverTools) } : null
      try { return await this.action(action, data) }
      catch (error) {
        if (before) { this.state = before.state; this.tokens = before.tokens; this.serverTools = before.tools }
        throw error
      }
    })
    this.tail = result.catch(() => {})
    return result
  }
  async save() {
    const path = join(this.root, 'inventory.json')
    await writeFile(`${path}.tmp`, JSON.stringify(this.state, null, 2), { mode: 0o600 })
    await rename(`${path}.tmp`, path)
  }
  async profile(item, exposure = 'codemode') {
    const env = { PATH: process.env.PATH, HOME: this.root, USERPROFILE: this.root,
      XDG_CONFIG_HOME: this.root, XDG_CACHE_HOME: this.root, APPDATA: this.root, LOCALAPPDATA: this.root,
      TMPDIR: this.root, TMP: this.root, TEMP: this.root, PI_CODING_AGENT_DIR: join(this.root, 'agent'), PI_OFFLINE: '1',
      ...(process.env.SystemRoot ? { SystemRoot: process.env.SystemRoot } : {}),
    }
    for (const [key, value] of Object.entries(process.env)) {
      if (/^DEMO_[A-Z0-9_]+$/.test(key) || ['DISPLAY', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS'].includes(key)) env[key] = value
    }
    const definition = { ...item.definition, exposure }
    const token = this.tokens.get(item.id)
    if (token) {
      env.DEMO_BEARER = token
      definition.headers = { ...definition.headers, Authorization: 'Bearer ${DEMO_BEARER}' }
    }
    await writeFile(join(this.root, 'agent/mcp.json'), JSON.stringify({ mcpServers: { [item.id]: definition } }), { mode: 0o600 })
    await writeFile(join(this.root, 'agent/settings.json'), JSON.stringify({ defaultTools: ['codemode'] }), { mode: 0o600 })
    return { cwd: join(this.root, 'work'), env }
  }
  item(id) {
    const item = this.state.items.find(item => item.id === id)
    if (!item) throw new Error('The server does not exist. Refresh the page.')
    return item
  }
  async connect(item, auth = false) {
    item.lastCheck = { status: 'failed', tools: 0 }
    this.serverTools.delete(item.id)
    await this.save()
    const options = await this.profile(item)
    if (auth) {
      const login = await runPi(['mcp', 'login', item.id, '--timeout', '90'], { ...options, timeout: 95000 })
      if (login.code !== 0) throw new Error('Sign-in failed. Check the provider requirements and try again. Browser sign-in needs a local desktop.')
    }
    let result
    try { result = await runPi(['mcp', 'list', '--json'], options) }
    catch { item.lastCheck = { status: 'failed', tools: 0 }; await this.save(); throw new Error(connectionError) }
    let list
    try { list = JSON.parse(result.stdout) } catch { throw new Error(connectionError) }
    const server = list.servers?.find(server => server.name === item.id)
    if (result.code !== 0 || server?.state !== 'connected') {
      item.lastCheck = { status: 'failed', tools: 0 }
      await this.save()
      throw new Error(connectionError)
    }
    item.lastCheck = { status: 'connected', tools: server.tools.length }
    this.serverTools.set(item.id, server.tools)
    await this.save()
    return { status: 'connected', tools: server.tools.map(name => ({ name })) }
  }
  async action(action, data) {
    if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('Enter an action object.')
    if (action === 'read') return structuredClone(this.state)
    if (action === 'server') {
      if (typeof data.name !== 'string' || !data.name.trim() || data.name.length > 100) throw new Error('Enter a name of 1 to 100 characters.')
      const definition = validateDefinition(data.definition)
      if (data.token !== undefined && typeof data.token !== 'string') throw new Error('Enter a text bearer token.')
      const old = data.id ? this.item(data.id) : this.state.items.find(item => data.source && item.source === data.source)
      if (data.token && (!definition.url || typeof data.token !== 'string' || data.token.length > 8192 || /[\r\n]/.test(data.token))) throw new Error('Use a bearer token for an HTTP server only.')
      if (data.token && (data.definition.auth === 'oauth' || Object.keys(definition.headers || {}).some(key => key.toLowerCase() === 'authorization'))) throw new Error('Choose one authentication method.')
      const item = { id: old?.id || `demo-${randomUUID()}`, name: data.name.trim(), kind: 'mcp',
        source: typeof data.source === 'string' ? data.source : '', description: typeof data.description === 'string' ? data.description : '',
        definition, enabled: old?.enabled ?? true }
      // A new endpoint must never inherit credentials from the old endpoint.
      if (old?.definition.url !== definition.url || data.definition.auth === 'oauth' || Object.keys(definition.headers || {}).some(key => key.toLowerCase() === 'authorization')) this.tokens.delete(item.id)
      if (data.token) this.tokens.set(item.id, data.token)
      this.state.items = [...this.state.items.filter(entry => entry.id !== item.id), item]
      this.serverTools.delete(item.id)
      await this.save()
      return structuredClone(this.state)
    }
    if (action === 'toggle') {
      if (typeof data.enabled !== 'boolean') throw new Error('Choose an enabled state.')
      this.item(data.id).enabled = data.enabled
      await this.save()
      return structuredClone(this.state)
    }
    if (action === 'remove') {
      const item = this.item(data.id)
      if (item.definition.url && !this.state.items.some(other => other.id !== item.id && other.definition.url === item.definition.url)) {
        const result = await runPi(['mcp', 'logout', item.id], await this.profile(item))
        if (result.code !== 0) throw new Error('The demo could not clear its OAuth credentials. Try uninstalling again.')
      }
      this.state.items = this.state.items.filter(item => item.id !== data.id)
      this.tokens.delete(data.id)
      this.serverTools.delete(data.id)
      await this.save()
      return structuredClone(this.state)
    }
    if (!['test', 'auth', 'chat', 'app', 'app-call'].includes(action)) throw new Error('This demonstration supports MCP servers only.')
    const item = this.item(data.id)
    if (action === 'test' || action === 'auth') return this.connect(item, action === 'auth')
    if (action === 'chat' || action === 'app' || action === 'app-call') {
      if (data.approved !== true) throw new Error('Approve this tool call first.')
      if (!item.enabled) throw new Error('Enable this server before calling a tool.')
      if (!item.lastCheck || item.lastCheck.status !== 'connected') throw new Error('Test the connection before calling a tool.')
      if (action === 'chat') return this.chat(item, data)
      if (action === 'app-call' && data.tool !== 'quote') throw new Error('The comparison app can call only quote.')
      return this.app(item, data)
    }
    throw new Error('This demonstration supports MCP servers only.')
  }
  async chat(item, data) {
    if (!['direct', 'codemode'].includes(data.exposure)) throw new Error('Choose direct or codemode exposure.')
    if (typeof data.tool !== 'string' || !this.serverTools.get(item.id)?.includes(data.tool)) throw new Error('Choose a tool from the connected server.')
    if (!data.args || typeof data.args !== 'object' || Array.isArray(data.args)) throw new Error('Enter a JSON object for tool arguments.')
    const options = await this.profile(item, data.exposure)
    // Match the candidate naming function, including its length hash.
    const toolsPath = new URL('./extensions/mcp/tools.js', import.meta.resolve('@earendil-works/pi-coding-agent')).href
    const { createMcpToolName } = await import(toolsPath)
    const names = new Set()
    let tool
    for (const name of this.serverTools.get(item.id)) {
      const registered = createMcpToolName(item.id, name, candidate => names.has(candidate))
      names.add(registered)
      if (name === data.tool) tool = registered
    }
    const code = `const result = await tools[${JSON.stringify(tool.replaceAll('-', '_'))}](${JSON.stringify(data.args)}); text(result);`
    options.env.MUNIMENT_DEMO_APPROVAL = JSON.stringify({ tool, args: data.args, code })
    await writeFile(join(this.root, 'agent/models.json'), JSON.stringify({ providers: { fixture: {
      baseUrl: this.modelUrl, api: 'openai-completions', apiKey: 'disposable',
      models: [{ id: 'fixture', contextWindow: 128000, maxTokens: 8192, input: ['text'] }],
    } } }), { mode: 0o600 })
    this.pendingModel = { tool: data.exposure === 'direct' ? tool : 'codemode', args: data.exposure === 'direct' ? data.args : { code }, requests: 0 }
    try {
      const result = await runPi([...chatFlags, `Call ${data.tool} with ${JSON.stringify(data.args)}.`], options)
      if (result.code !== 0 || /Failed to load extension|errors loading models/i.test(result.stderr)) throw new Error('The candidate chat failed. Check the server and retry.')
      const events = result.stdout.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line))
      if (!events.some(event => event.type === 'tool_execution_end')) throw new Error('The candidate returned no tool result. Check the connection and retry.')
      // Keep system prompts and diagnostic output off the browser wire.
      return { events: events.filter(event => ['tool_execution_start', 'tool_execution_end'].includes(event.type) || (event.type === 'message_end' && event.message.role !== 'system')) }
    } finally { this.pendingModel = null }
  }
  async app(item, data) {
    if (!isDeepStrictEqual(item.definition, validateDefinition(this.fixture.stdio)) && !isDeepStrictEqual(item.definition, validateDefinition(this.fixture.http))) throw new Error('The comparison app host accepts only the disposable fixture.')
    if (data.tool && data.tool !== 'quote') throw new Error('The comparison app can call only quote.')
    const client = new McpClient({ name: 'muniment-demo-comparison', version: '1.0.0', requestTimeoutMs: 10000 })
    const transport = item.definition.url
      ? new StreamableHttpTransport({ url: item.definition.url })
      : new StdioTransport({ command: item.definition.command, args: item.definition.args, cwd: join(this.root, 'work'), env: (await this.profile(item)).env, inheritEnv: false })
    try {
      await client.connect(transport)
      const result = await client.callTool('quote', data.args ?? { seats: 3 }, { signal: AbortSignal.timeout(10000) })
      if (data.tool) return result
      const tools = await client.listTools()
      const tool = tools.find(tool => tool.name === 'quote')
      const resource = await client.readResource(tool._meta.ui.resourceUri)
      return { tool, result, resource }
    } finally { await client.close() }
  }
  async close() {
    await this.tail
    await rm(this.root, { recursive: true, force: true })
  }
}

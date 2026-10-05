import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile, writeFile, mkdir, access } from 'node:fs/promises'
import { join } from 'node:path'
import { startDemo } from './server.mjs'
import { validateDefinition, runPi, chatFlags } from './boundary.mjs'

const save = (demo, definition = demo.fixture.stdio, name = 'Workshop') => demo.boundary.dispatch('server', { name, definition })
const call = (id, revision, exposure = 'direct', args = { seats: 3 }) => ({ id, revision, approved: true, tool: 'quote', exposure, args })

test('freezes registry integrity and one Pi version across the probe graph', async () => {
  const manifest = JSON.parse(await readFile(new URL('./package.json', import.meta.url), 'utf8'))
  const lock = JSON.parse(await readFile(new URL('./package-lock.json', import.meta.url), 'utf8'))
  assert.deepEqual(lock.packages[''].dependencies, manifest.dependencies)
  for (const [path, pkg] of Object.entries(lock.packages)) {
    if (!path) continue
    assert.match(pkg.resolved, /^https:\/\/registry\.npmjs\.org\//, path)
    assert.match(pkg.integrity, /^sha512-/, path)
    const name = path.split('node_modules/').at(-1)
    if (/^@earendil-works\/(pi-|chord)/.test(name)) assert.equal(pkg.version, '0.99.1', path)
    assert.notEqual(name, 'pi-mcp-adapter')
  }
})

test('rejects invalid and contradictory configurations without executing them', () => {
  for (const value of [null, [], {}, { command: 'node', url: 'https://example.com' },
    { command: '', url: 'https://example.com' }, { url: null }, { command: 'node', type: null },
    { url: 'https://example.com', args: null },
    { url: 'file:///private' }, { url: 'https://user:secret@example.com' },
    { type: 'sse', url: 'https://example.com' }, { command: 'node', args: [1] },
    { command: 'node', env: [] }, { command: 'node', env: { SECRET: '${REAL_SECRET}' } },
    { command: 'node', env: { SECRET: '!cat ~/.secret' } },
    { url: 'https://example.com', headers: { Authorization: 'private' } },
    { url: 'https://example.com', type: 'stdio' }, { command: 'node', type: 'http' },
    { command: 'node', timeout: 0 }, { command: 'node', timeout: Infinity },
    { command: 'node', timeout: '10' }, { command: 'node', exposure: 'hidden' },
    { command: 'node', socket: '/tmp/server' }, { url: 'https://example.com', oauth: { callbackPort: 0 } },
  ]) assert.throws(() => validateDefinition(value), JSON.stringify(value))
  assert.equal(validateDefinition({ command: 'node', env: { KEY: '${DEMO_KEY}' }, timeout: 1 }).timeout, 1)
  assert.equal(validateDefinition({ url: 'https://example.com', timeout: 30 }).exposure, 'codemode')
})

test('preserves custom entries, credentials, toggles, and failed connection state', { timeout: 60000 }, async () => {
  const demo = await startDemo({ ui: false })
  try {
    const [one, two] = await Promise.all([save(demo), save(demo, demo.fixture.http, 'HTTP workshop')])
    assert.equal(one.items.length, 1)
    assert.equal(two.items.length, 2)
    const id = one.items[0].id
    assert.equal((await demo.boundary.dispatch('test', { id })).tools[0].name, 'quote')
    assert.equal((await demo.boundary.dispatch('read')).items.length, 2)
    const saveState = demo.boundary.save
    demo.boundary.save = async () => { throw new Error('The fixture rejected the disk write.') }
    try {
      await assert.rejects(demo.boundary.dispatch('toggle', { id, enabled: false }), /disk write/)
      assert.equal((await demo.boundary.dispatch('read')).items[0].enabled, true)
    } finally { demo.boundary.save = saveState }
    await demo.boundary.dispatch('toggle', { id, enabled: false })
    await assert.rejects(demo.boundary.dispatch('chat', call(id)), /Enable/)
    await demo.boundary.dispatch('toggle', { id, enabled: true })
    await assert.rejects(demo.boundary.dispatch('chat', { ...call(id), approved: false }), /Approve/)
    await assert.rejects(demo.boundary.dispatch('server', { name: ' ', definition: demo.fixture.stdio }), /name/)
    await assert.rejects(demo.boundary.dispatch('server', { id: 'missing', name: 'Missing', definition: demo.fixture.stdio }), /does not exist/)
    await assert.rejects(demo.boundary.dispatch('toggle', { id, enabled: 'false' }), /enabled state/)
    const broken = (await save(demo, { command: 'muniment-demo-command-does-not-exist' }, 'Broken')).items.at(-1)
    await assert.rejects(demo.boundary.dispatch('test', { id: broken.id }), /Connection failed/)
    let persisted = JSON.parse(await readFile(join(demo.boundary.root, 'inventory.json'), 'utf8'))
    assert.equal(persisted.items.find(item => item.id === broken.id).lastCheck.status, 'failed')
    const secret = 'disposable-test-token'
    const secured = (await demo.boundary.dispatch('server', { name: 'Token', definition: { url: 'https://example.com/mcp' }, token: secret })).items.at(-1)
    assert.ok(!JSON.stringify(await demo.boundary.dispatch('read')).includes(secret))
    assert.ok(!(await readFile(join(demo.boundary.root, 'inventory.json'), 'utf8')).includes(secret))
    await demo.boundary.dispatch('server', { id: secured.id, name: 'Token', definition: { url: 'https://example.com/mcp' } })
    assert.equal(demo.boundary.tokens.get(secured.id), secret)
    await demo.boundary.dispatch('server', { id: secured.id, name: 'Token', definition: { url: 'https://example.com/mcp', auth: 'oauth' } })
    assert.equal(demo.boundary.tokens.has(secured.id), false)
    await demo.boundary.dispatch('server', { id: secured.id, name: 'Token', definition: { url: 'https://example.com/mcp' }, token: secret })
    await demo.boundary.dispatch('server', { id: secured.id, name: 'Token', definition: { url: 'https://other.example/mcp' } })
    assert.equal(demo.boundary.tokens.has(secured.id), false)
    await demo.boundary.dispatch('remove', { id: secured.id })
    persisted = JSON.parse(await readFile(join(demo.boundary.root, 'inventory.json'), 'utf8'))
    assert.equal(persisted.items.length, 3)
    await assert.rejects(demo.boundary.dispatch('chat', call(secured.id)), /does not exist/)
  } finally { const root = demo.boundary.root; await demo.close(); await assert.rejects(access(root)) }
})

test('binds tool approval to the connected server revision', { timeout: 120000 }, async () => {
  const demo = await startDemo({ ui: false })
  try {
    const id = (await save(demo)).items[0].id
    const first = await demo.boundary.dispatch('test', { id })
    assert.equal(typeof first.revision, 'string')
    const stale = call(id, first.revision)
    const rejectStale = async () => {
      for (const action of ['chat', 'app', 'app-call']) {
        await assert.rejects(demo.boundary.dispatch(action, stale), /server changed/)
      }
    }
    await demo.boundary.dispatch('server', { id, name: 'Replacement', definition: demo.fixture.http })
    let connected = await demo.boundary.dispatch('test', { id })
    assert.notEqual(connected.revision, first.revision)
    await rejectStale()
    for (const revision of [undefined, null, '', 1, {}]) {
      await assert.rejects(demo.boundary.dispatch('chat', call(id, revision)), /server changed/)
    }
    assert.equal((await demo.boundary.dispatch('app-call', call(id, connected.revision))).structuredContent.total, 75)
    for (const token of ['disposable-first', 'disposable-replacement']) {
      stale.revision = connected.revision
      await demo.boundary.dispatch('server', { id, name: 'Replacement', definition: demo.fixture.http, token })
      connected = await demo.boundary.dispatch('test', { id })
      assert.notEqual(connected.revision, stale.revision)
      await rejectStale()
    }
    stale.revision = connected.revision
    await demo.boundary.dispatch('server', { id, name: 'Replacement', definition: { ...demo.fixture.http, auth: 'oauth' } })
    connected = await demo.boundary.dispatch('test', { id })
    await rejectStale()
    stale.revision = connected.revision
    connected = await demo.boundary.dispatch('auth', { id })
    assert.notEqual(connected.revision, stale.revision)
    await rejectStale()
    stale.revision = connected.revision
    await demo.boundary.dispatch('toggle', { id, enabled: false })
    await demo.boundary.dispatch('toggle', { id, enabled: true })
    await rejectStale()
    connected = await demo.boundary.dispatch('test', { id })
    await assert.rejects(demo.boundary.dispatch('chat', { ...call(id, connected.revision), approved: false }), /Approve/)
    const result = await demo.boundary.dispatch('chat', call(id, connected.revision))
    assert.match(JSON.stringify(result.events), /3 workshop seats cost/)
  } finally { await demo.close() }
})

test('invalidates shared OAuth approvals before sign-in through another entry', { timeout: 120000 }, async () => {
  const demo = await startDemo({ ui: false })
  try {
    const first = (await save(demo, demo.fixture.http, 'First account')).items.at(-1)
    const second = (await save(demo, demo.fixture.http, 'Second account')).items.at(-1)
    const unrelated = (await save(demo)).items.at(-1)
    const unaffected = await demo.boundary.dispatch('test', { id: unrelated.id })
    const authPath = join(demo.boundary.root, 'agent/mcp-auth.json')
    for (const fail of [false, true]) {
      const connected = await demo.boundary.dispatch('test', { id: first.id })
      const other = await demo.boundary.dispatch('test', { id: second.id })
      if (fail) await writeFile(authPath, '{invalid')
      const signingIn = demo.boundary.dispatch('auth', { id: second.id })
      if (fail) await assert.rejects(signingIn, /Sign-in failed/)
      else assert.equal((await signingIn).status, 'connected')
      const state = await demo.boundary.dispatch('read')
      const invalidated = state.items.find(item => item.id === first.id)
      assert.notEqual(invalidated.revision, connected.revision)
      assert.equal(invalidated.lastCheck.status, 'failed')
      assert.equal(demo.boundary.serverTools.has(first.id), false)
      assert.notEqual(state.items.find(item => item.id === second.id).revision, other.revision)
      if (fail) {
        assert.equal(state.items.find(item => item.id === second.id).lastCheck.status, 'failed')
        assert.equal(demo.boundary.serverTools.has(second.id), false)
      }
      assert.equal(state.items.find(item => item.id === unrelated.id).revision, unaffected.revision)
      assert.equal(state.items.find(item => item.id === unrelated.id).lastCheck.status, 'connected')
      assert.ok(demo.boundary.serverTools.has(unrelated.id))
      assert.deepEqual(JSON.parse(await readFile(join(demo.boundary.root, 'inventory.json'), 'utf8')), state)
      for (const action of ['chat', 'app', 'app-call']) {
        await assert.rejects(demo.boundary.dispatch(action, call(first.id, connected.revision)), /server changed/)
        await assert.rejects(demo.boundary.dispatch(action, call(first.id, invalidated.revision)), /Test the connection/)
      }
      if (fail) await writeFile(authPath, '{}')
      const fresh = await demo.boundary.dispatch('test', { id: first.id })
      for (const action of ['chat', 'app', 'app-call']) {
        await assert.rejects(demo.boundary.dispatch(action, call(first.id, connected.revision)), /server changed/)
        await assert.rejects(demo.boundary.dispatch(action, { ...call(first.id, fresh.revision), approved: false }), /Approve/)
      }
      assert.match(JSON.stringify((await demo.boundary.dispatch('chat', call(first.id, fresh.revision))).events), /3 workshop seats cost/)
      const app = await demo.boundary.dispatch('app', { id: first.id, revision: fresh.revision, approved: true })
      assert.equal(app.result.structuredContent.total, 75)
      assert.equal((await demo.boundary.dispatch('app-call', call(first.id, fresh.revision))).structuredContent.total, 75)
    }
  } finally { await demo.close() }
})

test('removes bearer tokens and Authorization references through URL-scoped OAuth logout', { timeout: 120000 }, async () => {
  const demo = await startDemo({ ui: false })
  const previous = process.env.DEMO_REMOVE_AUTH
  try {
    const authPath = join(demo.boundary.root, 'agent/mcp-auth.json')
    const unrelated = { 'https://other.example/mcp': { tokens: { access_token: 'disposable-unrelated' } } }
    const credentials = { ...unrelated, [demo.fixture.http.url]: { tokens: { access_token: 'disposable-oauth' } } }
    for (const header of [null, 'Authorization', 'aUtHoRiZaTiOn']) {
      process.env.DEMO_REMOVE_AUTH = 'Bearer disposable-reference'
      const definition = header ? { ...demo.fixture.http, headers: { [header]: '${DEMO_REMOVE_AUTH}' } } : demo.fixture.http
      const token = header ? undefined : 'disposable-bearer'
      const item = (await demo.boundary.dispatch('server', { name: 'HTTP workshop', definition, token })).items.at(-1)
      await demo.boundary.dispatch('test', { id: item.id })
      delete process.env.DEMO_REMOVE_AUTH
      await writeFile(authPath, JSON.stringify(credentials), { mode: 0o600 })
      await demo.boundary.dispatch('remove', { id: item.id })
      assert.equal((await demo.boundary.dispatch('read')).items.length, 0)
      assert.equal(demo.boundary.tokens.has(item.id), false)
      assert.equal(demo.boundary.serverTools.has(item.id), false)
      assert.deepEqual(JSON.parse(await readFile(authPath, 'utf8')), unrelated)
    }
    const one = (await save(demo, demo.fixture.http)).items[0]
    const two = (await save(demo, demo.fixture.http)).items.at(-1)
    await writeFile(authPath, JSON.stringify(credentials))
    await demo.boundary.dispatch('remove', { id: one.id })
    assert.deepEqual(JSON.parse(await readFile(authPath, 'utf8')), credentials)
    await demo.boundary.dispatch('remove', { id: two.id })
    assert.deepEqual(JSON.parse(await readFile(authPath, 'utf8')), unrelated)

    const item = (await demo.boundary.dispatch('server', { name: 'Retry', definition: demo.fixture.http, token: 'disposable-retry' })).items[0]
    await demo.boundary.dispatch('test', { id: item.id })
    const before = await demo.boundary.dispatch('read')
    await writeFile(authPath, '{invalid')
    await assert.rejects(demo.boundary.dispatch('remove', { id: item.id }), /could not clear its OAuth credentials/)
    assert.deepEqual(await demo.boundary.dispatch('read'), before)
    assert.equal(demo.boundary.tokens.get(item.id), 'disposable-retry')
    assert.ok(demo.boundary.serverTools.has(item.id))
    assert.equal(await readFile(authPath, 'utf8'), '{invalid')
    await writeFile(authPath, JSON.stringify(credentials))
    await demo.boundary.dispatch('remove', { id: item.id })
    assert.equal((await demo.boundary.dispatch('read')).items.length, 0)
    assert.deepEqual(JSON.parse(await readFile(authPath, 'utf8')), unrelated)
  } finally {
    if (previous === undefined) delete process.env.DEMO_REMOVE_AUTH
    else process.env.DEMO_REMOVE_AUTH = previous
    await demo.close()
  }
})

test('runs real candidate chat and exposes the app metadata loss on both transports', { timeout: 120000 }, async () => {
  const demo = await startDemo({ ui: false })
  try {
    for (const definition of [demo.fixture.stdio, demo.fixture.http]) {
      const id = (await save(demo, definition)).items.at(-1).id
      let { revision } = await demo.boundary.dispatch('test', { id })
      const app = await demo.boundary.dispatch('app', { id, revision, approved: true, args: { seats: 3 } })
      assert.equal(app.tool._meta.ui.resourceUri, 'ui://muniment-demo/quote.html')
      assert.equal(app.result.structuredContent.total, 75)
      assert.match(app.resource.contents[0].text, /Update quote/)
      for (const exposure of ['direct', 'codemode']) {
        const result = await demo.boundary.dispatch('chat', call(id, revision, exposure))
        const ends = result.events.filter(event => event.type === 'tool_execution_end')
        assert.ok(ends.length >= 1)
        assert.ok(ends.every(event => !event.isError), JSON.stringify(ends))
        assert.match(JSON.stringify(ends), /3 workshop seats cost \$75/)
        assert.doesNotMatch(JSON.stringify(ends), /fixtureMarker|ui:\/\//)
        assert.equal(result.events.at(-1).message.stopReason, 'stop')
      }
      const update = await demo.boundary.dispatch('app-call', { id, revision, approved: true, tool: 'quote', args: { seats: 5 } })
      assert.equal(update.structuredContent.total, 125)
      if (definition.url) {
        const auth = await demo.boundary.dispatch('auth', { id })
        assert.equal(auth.status, 'connected')
        revision = auth.revision
      }
      const failed = await demo.boundary.dispatch('chat', call(id, revision, 'direct', { seats: 3, fail: true }))
      assert.equal(failed.events.find(event => event.type === 'tool_execution_end').isError, true)
      for (const seats of [0, 21, 1.5]) {
        const invalid = await demo.boundary.dispatch('app-call', { id, revision, approved: true, tool: 'quote', args: { seats } })
        assert.equal(invalid.isError, true)
      }
      await assert.rejects(demo.boundary.dispatch('app-call', { id, revision, approved: true, tool: 'bash', args: {} }), /only quote/)
    }
  } finally { await demo.close() }
})

test('blocks cross-origin control and keeps project configuration untrusted', { timeout: 60000 }, async () => {
  const demo = await startDemo({ ui: false })
  try {
    const response = await fetch(`${demo.origin}/api/command`, { method: 'POST', headers: { origin: 'https://attacker.example', 'x-demo-token': demo.token }, body: '{}' })
    assert.equal(response.status, 403)
    assert.equal((await fetch(`${demo.origin}/api/info`)).status, 403)
    const item = (await save(demo)).items[0]
    const options = await demo.boundary.profile(item)
    assert.equal((await runPi(['--version'], options)).stdout.trim(), '0.99.1')
    assert.notEqual(options.env.HOME, process.env.HOME)
    assert.equal(options.env.ANTHROPIC_API_KEY, undefined)
    assert.equal(options.env.PI_MCP_CONFIG_MODE, undefined)
    await assert.rejects(runPi(['mcp', 'list', '--json'], { ...options, timeout: 1 }), /timed out/)
    await mkdir(join(options.cwd, '.pi'))
    await writeFile(join(options.cwd, '.pi/mcp.json'), JSON.stringify({ mcpServers: { untrusted: { command: 'must-not-run' } } }))
    const result = await runPi(['mcp', 'list', '--json'], options)
    assert.equal(result.code, 0)
    const list = JSON.parse(result.stdout)
    assert.deepEqual(list.servers.map(server => server.name), [item.id])
    assert.equal(list.servers[0].resources, 0)
    assert.match(list.note, /trust/i)
    const { revision } = await demo.boundary.dispatch('test', { id: item.id })
    const chat = await demo.boundary.dispatch('chat', call(item.id, revision))
    const tool = chat.events.find(event => event.type === 'tool_execution_end').toolName
    const deniedOptions = await demo.boundary.profile(item, 'direct')
    deniedOptions.env.MUNIMENT_DEMO_APPROVAL = JSON.stringify({ tool, args: { seats: 2 }, code: '' })
    demo.boundary.pendingModel = { tool, args: { seats: 3 }, requests: 0 }
    try {
      const denied = await runPi([...chatFlags, 'Try the unapproved arguments.'], deniedOptions)
      assert.equal(denied.code, 0)
      const events = denied.stdout.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line))
      const result = events.find(event => event.type === 'tool_execution_end')
      assert.equal(result.isError, true)
      assert.match(JSON.stringify(result), /did not approve/)
      assert.doesNotMatch(JSON.stringify(result), /seats cost/)
    } finally { demo.boundary.pendingModel = null }
  } finally { await demo.close() }
})

<script>
  import { onMount, tick } from 'svelte'
  import ExtendSection from '@desktop/extend/ExtendSection.svelte'
  import { AppBridge, PostMessageTransport } from '@modelcontextprotocol/ext-apps/app-bridge'
  let info = $state(null), items = $state([]), selected = $state(''), tools = $state([]), tool = $state('quote')
  let args = $state('{"seats":3}'), exposure = $state('codemode'), approved = $state(false)
  let busy = $state(false), error = $state(''), events = $state([]), appData = $state(null), frame = $state(), bridge
  let appCalls = $state(0), appReady = $state(false)
  let connectionBusy = $state(false)
  const token = document.querySelector('meta[name=demo-token]').content
  const reply = $derived(events.filter(event => event.type === 'message_end' && event.message.role === 'assistant').at(-1)?.message.content.filter(block => block.type === 'text').map(block => block.text).join('\n'))
  const results = $derived(events.filter(event => event.type === 'tool_execution_end'))
  async function request(action, data = {}) {
    const response = await fetch('/api/command', { method: 'POST', headers: { 'content-type': 'application/json', 'x-demo-token': token }, body: JSON.stringify({ action, data }) })
    const value = await response.json()
    if (!response.ok) throw new Error(value.error)
    return value.result
  }
  async function refresh() {
    items = (await request('read')).items
    if (!items.some(item => item.id === selected)) selected = items[0]?.id || ''
  }
  async function closeApp() { const closing = bridge; bridge = null; appData = null; appReady = false; await closing?.close() }
  const tauri = { invoke: async (_, { action, data }) => {
    if (action === 'server' && data.token && !window.confirm('This demo keeps the bearer token in memory only. Use a disposable token. Save it?')) throw new Error('The demo did not save the token.')
    if (action === 'server' && (data.definition.command || data.source) && !window.confirm('This server can run code or contact a provider. Save it in the disposable demo profile?')) throw new Error('The demo did not save the server.')
    if (['auth', 'test'].includes(action) && !window.confirm('Connect to this server from the disposable demo profile?')) throw new Error('The demo did not connect to the server.')
    connectionBusy = true
    try { return await request(action, data) }
    finally { try { await refresh(); if (action !== 'read') { tools = []; approved = false; await closeApp() } } finally { connectionBusy = false } }
  } }
  async function work(fn) { busy = true; error = ''; try { await fn() } catch (e) { error = e.message } finally { busy = false } }
  async function connect() {
    await closeApp(); approved = false; tools = []
    const result = await request('test', { id: selected })
    tools = result.tools; tool = tools[0]?.name || ''; await refresh()
  }
  async function chat() {
    events = []
    const result = await request('chat', { id: selected, tool, args: JSON.parse(args), exposure, approved })
    events = result.events
  }
  async function openApp() {
    await closeApp()
    const openedId = selected, openedArgs = JSON.parse(args)
    const opened = await request('app', { id: openedId, args: openedArgs, approved })
    appData = opened; appCalls = 0
    await tick()
    const nextBridge = new AppBridge(null, { name: 'Muniment comparison host', version: '0.0.1' }, { serverTools: {} }, { hostContext: { theme: 'light' } })
    bridge = nextBridge
    nextBridge.oncalltool = async params => {
      if (!approved || selected !== openedId || bridge !== nextBridge) throw new Error('Approve this tool call first.')
      const result = await request('app-call', { id: openedId, tool: params.name, args: params.arguments, approved })
      if (bridge === nextBridge) appCalls++
      return result
    }
    nextBridge.oninitialized = () => {
      if (bridge !== nextBridge) return
      appReady = true
      void nextBridge.sendToolInput({ arguments: openedArgs })
      void nextBridge.sendToolResult(opened.result)
    }
    // An opaque-origin iframe isolates this single, allowlisted fixture. This is not a general app host.
    await nextBridge.connect(new PostMessageTransport(frame.contentWindow, frame.contentWindow))
    frame.srcdoc = opened.resource.contents[0].text
  }
  onMount(() => {
    void work(async () => { info = await (await fetch('/api/info', { headers: { 'x-demo-token': token } })).json(); await refresh() })
    return () => { void bridge?.close() }
  })
</script>

<main>
  <header><div><p class="eyebrow">Muniment / Local review</p><h1>MCP comparison</h1></div><span class="version">Pi 0.99.1 · Disposable profile</span></header>
  <aside class="notice"><strong>Production stays unchanged.</strong> This demo uses the real Extend component and candidate MCP calls.
    <p>The local model fixture chooses one approved tool call. It does not use a model account.</p>
    <p>Tokens stay in memory. OAuth credentials stay in the disposable profile, not the system credential store.</p>
    <p>Skills and plugins stay outside this MCP demo.</p>
  </aside>
  <div class="layout">
    <section class="catalog" aria-label="Settings Extend"><h2>Settings → Extend</h2><fieldset disabled={busy}><ExtendSection {tauri} /></fieldset></section>
    <section class="comparison" aria-label="Tool comparison">
      <h2>Compare the same tool</h2>
      <p class="muted">Save a Custom server on the left. Then connect here.</p>
      {#if info}
        <details><summary>Show disposable fixture configuration</summary>
          <p>Use the name Workshop in Custom. Paste this JSON into the advanced configuration.</p>
          <pre data-testid="fixture-config">{JSON.stringify(info.fixture.stdio, null, 2)}</pre>
          <p>For streamable HTTP, use this server URL.</p><code>{info.fixture.http.url}</code>
          <p>Use only disposable servers. Local commands run with your operating system permissions.</p>
        </details>
      {/if}
      <fieldset class="controls" disabled={connectionBusy}>
        <label>Saved server<select aria-label="Saved server" bind:value={selected} disabled={busy} onchange={() => { tools = []; approved = false; events = []; void closeApp() }}><option value="">Choose a server</option>{#each items as item}<option value={item.id}>{item.name}{item.enabled ? '' : ' (disabled)'}</option>{/each}</select></label>
        <button disabled={busy || !selected} onclick={() => work(connect)}>Connect and list tools</button>
        <label>Tool<select aria-label="Tool" bind:value={tool} disabled={busy || !tools.length} onchange={() => approved = false}>{#each tools as entry}<option value={entry.name}>{entry.name}</option>{/each}</select></label>
        <label>Tool arguments<textarea bind:value={args} rows="2" disabled={busy} oninput={() => approved = false}></textarea></label>
        <label>Candidate exposure<select aria-label="Candidate exposure" bind:value={exposure} disabled={busy}><option value="codemode">Codemode (default)</option><option value="direct">Direct</option></select></label>
        <label class="approval"><input type="checkbox" bind:checked={approved} disabled={busy || !tools.length} onchange={() => { if (!approved) void closeApp() }}>Allow this tool call and fixture quote updates.</label>
        <div class="actions"><button disabled={busy || !approved || !tools.length} onclick={() => work(chat)}>Run candidate chat</button><button disabled={busy || !approved || !tools.length} onclick={() => work(openApp)}>Open comparison app</button></div>
      </fieldset>
      {#if busy}<p role="status">The candidate is working.</p>{/if}
      {#if error}<p role="alert">{error}</p>{/if}
      <article class="result"><p class="eyebrow">01 / Real candidate path</p><h3>Ordinary chat result</h3>
        {#if reply}<p class="reply">{reply}</p>{:else}<p class="muted">Run the candidate chat to see its tool result.</p>{/if}
        {#if results.length}<details><summary>Inspect candidate events</summary><pre>{JSON.stringify(events, null, 2)}</pre></details>{/if}
        <p class="boundary">Pi removes result _meta and does not render MCP apps.</p>
      </article>
      <article class="result"><p class="eyebrow">02 / Separate comparison path</p><h3>Interactive fixture</h3>
        <p class="muted">This host reads the fixture resource directly. It does not restore metadata through Pi chat.</p>
        {#if appData}<iframe title="Workshop MCP app" sandbox="allow-scripts" bind:this={frame}></iframe>
          <p class="receipt" role="status">{appReady ? 'The app connected.' : 'The app awaits initialization.'} Tool calls from the app: {appCalls}.</p>
          <button onclick={() => closeApp()}>Close app</button>
          <details><summary>Inspect original MCP metadata</summary><pre>{JSON.stringify({ tool: appData.tool, result: appData.result }, null, 2)}</pre></details>
        {:else}<p class="muted">Open the comparison app to change the seat count.</p>{/if}
      </article>
      <p class="footnote">This comparison does not prove native desktop integration or select a permanent app host.</p>
    </section>
  </div>
</main>
<style>
  :global(body) { margin: 0; background: var(--paper); color: var(--ink); font: var(--text-15)/var(--leading-body) var(--font-human); }
  :global(button:focus-visible), :global(button:hover) { background: var(--faint); }
  main { max-width: 1480px; margin: auto; padding: 32px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 20px; margin-bottom: 24px; }
  h1 { font-size: var(--text-28); margin: 4px 0 0; } h2 { font-size: var(--text-22); margin: 0 0 16px; } h3 { font-size: var(--text-17); margin: 4px 0 12px; }
  p { margin: 8px 0; } .eyebrow, .version, .receipt, .footnote { color: var(--muted); font: var(--text-12)/var(--leading-body) var(--font-mono); }
  .notice { border: 1px solid var(--border); border-radius: var(--radius-panel); padding: 16px 20px; margin-bottom: 24px; }
  .notice p { font-size: var(--text-13); margin: 4px 0 0; color: var(--muted); }
  .layout { display: grid; grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr); gap: 24px; align-items: start; }
  .catalog { max-height: 1100px; overflow: auto; }
  .catalog, .comparison { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); padding: 24px; min-width: 0; }
  .muted, .boundary { color: var(--muted); font-size: var(--text-13); }
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  .controls { display: grid; gap: 12px; margin: 20px 0; }
  label { display: grid; gap: 6px; font-size: var(--text-13); }
  select, textarea, button { font: inherit; color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px 10px; min-width: 0; }
  button { background: var(--faint); cursor: pointer; min-height: 32px; font-size: var(--text-13); }
  button:disabled { opacity: .5; cursor: default; }
  button:hover, button:focus-visible { background: var(--border); }
  .approval { display: flex; align-items: center; gap: 8px; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; }
  .result { border-top: 1px solid var(--border); padding: 20px 0 0; margin-top: 20px; }
  .reply { white-space: pre-wrap; overflow-wrap: anywhere; }
  details { margin-top: 12px; font-size: var(--text-13); } summary { cursor: pointer; padding: 4px 0; }
  pre, code, textarea { font: var(--text-12)/var(--leading-body) var(--font-mono); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 280px; overflow: auto; padding: 12px; background: var(--paper); }
  code { overflow-wrap: anywhere; } iframe { width: 100%; height: 330px; border: 1px solid var(--border); border-radius: var(--radius-panel); box-sizing: border-box; margin-top: 12px; }
  [role=alert] { color: var(--oxide); } .footnote { margin-top: 20px; }
  @media (max-width: 900px) { .layout { grid-template-columns: 1fr; } main { padding: 16px; } header { align-items: start; flex-direction: column; } }
</style>

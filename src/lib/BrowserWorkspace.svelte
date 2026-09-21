<script>
  import { onMount, tick } from 'svelte'
  import template from './artifact-template.html?raw'
  import LucideIcon from './LucideIcon.svelte'
  let { tauri, artifacts = false, suspended = false, onclose, onurl } = $props()
  let host = $state()
  let address = $state('https://example.org')
  let status = $state('')
  let allowed = $state(false)
  let items = $state([])
  let draft = $state({ id: null, name: '', html: template })
  let preview = $state(false)
  let busy = $state(false)
  let pageText = $state('')
  let active = true
  let queue = Promise.resolve()
  const view = $derived(artifacts ? 'artifact' : 'browser')
  const visible = $derived(!suspended && (!artifacts || preview))
  function layout() {
    queue = queue.catch(() => {}).then(async () => {
      if (!active || !visible || !host) return tauri.invoke('browser_view', { label: null, bounds: null, artifactId: null })
      const r = host.getBoundingClientRect()
      if (r.width < 1 || r.height < 1) return
      await tauri.invoke('browser_view', { label: view, bounds: { x: r.x, y: r.y, width: r.width, height: r.height }, artifactId: null })
    }).catch(e => { if (active) status = String(e) })
    return queue
  }
  async function action(action, value = '') {
    try {
      status = ''
      const result = await tauri.invoke('browser_command', { request: { view, action, value } })
      if (action === 'status' && /^https?:/.test(result.url)) { address = result.url; onurl?.(result.url) }
      if (action === 'grant') allowed = true
      if (action === 'stop' || action === 'navigate') allowed = false
      if (action === 'snapshot') pageText = JSON.parse(result).text
    } catch (e) { status = String(e) }
  }
  async function refresh() { items = await tauri.invoke('artifact_list') }
  async function save() {
    busy = true
    try {
      draft = await tauri.invoke('artifact_save', { ...draft })
      await refresh()
      preview = true
      await tick()
      await layout()
      await tauri.invoke('browser_view', { label: 'artifact', bounds: bounds(), artifactId: draft.id })
      status = 'Saved'
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
  function bounds() { const r = host.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height } }
  async function edit(id) {
    try { preview = false; draft = await tauri.invoke('artifact_read', { id }); status = '' }
    catch (e) { status = String(e) }
  }
  async function importFile(event) {
    const file = event.target.files?.[0]
    if (!file) return
    if (file.size > 2000000) { status = 'Choose an HTML file under 2 MB.'; return }
    draft = { id: null, name: file.name.replace(/\.html?$/i, ''), html: await file.text() }
    preview = false
  }
  $effect(() => { if (!visible) allowed = false; host; view; void layout() })
  onMount(() => {
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(layout)
    if (host) observer?.observe(host)
    const resized = () => { void layout() }
    window.addEventListener('resize', resized)
    let unlisten
    let unlistenAction
    void window.__TAURI__?.event?.listen('browser-page-change', ({ payload }) => {
      if (payload.view === view) { allowed = false; if (!artifacts) { address = payload.url; onurl?.(payload.url) } }
    }).then(fn => { if (active) unlisten = fn; else fn() })
    void window.__TAURI__?.event?.listen('agent-action', ({ payload }) => {
      if (payload.action === 'stop') allowed = false
    }).then(fn => { if (active) unlistenAction = fn; else fn() })
    if (artifacts) void refresh().catch(e => { status = String(e) })
    else void layout().then(() => { if (active) return action('status') })
    return () => {
      active = false
      observer?.disconnect()
      window.removeEventListener('resize', resized)
      unlisten?.()
      unlistenAction?.()
      void layout()
    }
  })
</script>
<section class="browser-workspace" aria-label={artifacts ? 'Artifacts' : 'Browser'}>
  <header><h2>{artifacts ? (draft.id ? draft.name : 'Create an Artifact') : 'Browser'}</h2><button aria-label="Close browser panel" onclick={onclose}><LucideIcon name="x" /></button></header>
  {#if artifacts}
    <div class="artifact-tools">
      <select aria-label="Saved artifacts" onchange={e => { if (e.target.value) void edit(e.target.value) }}><option value="">Saved artifacts</option>{#each items as item}<option value={item.id}>{item.name}</option>{/each}</select>
      <button onclick={() => { draft = { ...draft, id: null, name: '' }; preview = false }}>New</button>
      <label class="import">Import HTML<input type="file" accept=".html,.htm,text/html" onchange={importFile} /></label>
      {#if preview}<button onclick={() => preview = false}>Edit</button>{/if}
    </div>
    {#if !preview}
      <form onsubmit={e => { e.preventDefault(); void save() }}>
        <label>Name<input required maxlength="120" bind:value={draft.name} placeholder="My artifact" /></label>
        <label class="source">HTML<textarea aria-label="Artifact HTML" bind:value={draft.html} spellcheck="false"></textarea></label>
        <div><button type="submit" disabled={busy}>{busy ? 'Saving…' : 'Save and preview'}</button></div>
      </form>
    {/if}
  {:else}
    <form class="address" onsubmit={e => { e.preventDefault(); void action('navigate', address.includes('://') ? address : `https://${address}`) }}>
      <button type="button" aria-label="Back" onclick={() => action('back')}><LucideIcon name="arrow-left" /></button>
      <button type="button" aria-label="Forward" onclick={() => action('forward')}><LucideIcon name="arrow-right" /></button>
      <button type="button" aria-label="Reload" onclick={() => action('reload')}><LucideIcon name="refresh-cw" /></button>
      <input aria-label="Website address" bind:value={address} /><button>Go</button>
    </form>
  {/if}
  {#if !artifacts || preview}
    <div class="control"><button onclick={() => action(allowed ? 'stop' : 'grant')}>{allowed ? 'Stop agent control' : 'Allow agent control'}</button><button onclick={() => action('snapshot')}>Read page</button><span>{allowed ? 'Agent control allowed for this page' : 'You are in control'}</span></div>
  {/if}
  {#if status}<p role="status">{status}</p>{/if}
  {#if pageText}<details><summary>Page text</summary><pre>{pageText}</pre></details>{/if}
  <div class:hidden={artifacts && !preview} class="browser-host" bind:this={host} aria-label="Web page"></div>
</section>
<style>
  .browser-workspace { grid-area: thread; display:flex; flex-direction:column; min-width:0; min-height:0; overflow:hidden; background:var(--surface); border:1px solid var(--border); border-radius:var(--radius-panel); padding:16px; gap:12px; }
  header,.address,.artifact-tools,.control { display:flex; align-items:center; gap:8px; }
  header { justify-content:space-between; } h2 { margin:0; font-size:var(--text-17); }
  input,textarea,select { background:var(--paper); color:var(--ink); border:1px solid var(--border); border-radius:var(--radius-control); padding:8px; }
  button,.import { padding:7px 10px; background:transparent; color:var(--ink); border:1px solid var(--border); border-radius:var(--radius-control); cursor:pointer; }
  .address input { flex:1; min-width:100px; } form:not(.address) { display:flex; flex:1; min-height:0; flex-direction:column; gap:12px; } label { display:flex; flex-direction:column; gap:6px; }
  .source { flex:1; min-height:150px; } textarea { flex:1; resize:none; font:var(--text-13) var(--font-mono); }
  .import input { display:none; } .control { flex-wrap:wrap; font-size:var(--text-12); } .control span { color:var(--muted); }
  .browser-host { flex:1; min-height:150px; } .hidden { display:none; } p { margin:0; } pre { max-height:150px; overflow:auto; white-space:pre-wrap; }
</style>

<script>
  import { onMount } from 'svelte'
  import { readHomepage, saveHomepage, DEFAULT_HOMEPAGE } from './browser-settings.js'
  import LucideIcon from './LucideIcon.svelte'
  let { tauri, artifacts = false, requestedArtifact = null, suspended = false, navigation = null, onnavigationhandled, onurl, onartifact } = $props()
  let homepage = $state(readHomepage())
  let homepageDraft = $state(homepage)
  let settingsOpen = $state(false)
  let settingsError = $state('')
  let host = $state()
  let address = $state('')
  let status = $state('')
  let items = $state([])
  let selected = $state('')
  let loading = $state(artifacts)
  let active = true
  let queue = Promise.resolve()
  let displayedArtifact = null
  $effect(() => { if (artifacts) onartifact?.(items.find(item => item.id === selected)?.name || '') })
  const view = $derived(artifacts ? 'artifact' : 'browser')
  const visible = $derived(!suspended && (!artifacts || !!selected))
  function layout() {
    queue = queue.catch(() => {}).then(async () => {
      if (!active || !visible || !host) return tauri.invoke('browser_view', { label: null, bounds: null, artifactId: null })
      const r = host.getBoundingClientRect()
      if (r.width < 1 || r.height < 1) return
      const artifactId = artifacts && selected !== displayedArtifact ? selected : null
      await tauri.invoke('browser_view', { label: view, bounds: { x: r.x, y: r.y, width: r.width, height: r.height }, artifactId, homepage })
      if (artifactId) displayedArtifact = artifactId
    }).catch(e => { if (active) status = String(e) })
    return queue
  }
  async function action(action, value = '') {
    try {
      status = ''
      const result = await tauri.invoke('browser_command', { request: { view, action, value } })
      if (action === 'status' && /^https?:/.test(result.url)) { address = result.url; onurl?.(result.url) }
    } catch (e) { status = String(e) }
  }
  function saveSettings(event) {
    event.preventDefault()
    try { homepage = saveHomepage(homepageDraft); homepageDraft = homepage; settingsError = ''; settingsOpen = false }
    catch (error) { settingsError = String(error.message ?? error) }
  }
  async function refresh() {
    try {
      items = await tauri.invoke('artifact_list')
      if (requestedArtifact && items.some(item => item.id === requestedArtifact)) selected = requestedArtifact
      else if (!items.some(item => item.id === selected)) selected = items[0]?.id ?? ''
    } catch (e) { status = String(e) }
    finally { loading = false }
  }
  function navigate(event) {
    event.preventDefault()
    const value = address.trim()
    if (value) void action('navigate', value.includes('://') ? value : `https://${value}`)
  }
  $effect(() => { if (artifacts && requestedArtifact) { selected=requestedArtifact; void layout() } })
  $effect(() => { visible; host; view; selected; void layout() })
  $effect(() => {
    const request = navigation
    if (!request || artifacts) return
    address = request.url
    void layout().then(() => action('navigate',request.url)).finally(() => onnavigationhandled?.(request))
  })
  onMount(() => {
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(layout)
    if (host) observer?.observe(host)
    const resized = () => { void layout() }
    window.addEventListener('resize', resized)
    const cleanups = []
    for (const [name, listener] of [
      ['browser-page-change', ({ payload }) => { if (payload.view === view && !artifacts) { address = payload.url; onurl?.(payload.url) } }],
      ['artifact-created', ({payload}) => { if (artifacts) { if (payload.id === selected) displayedArtifact = null; void refresh().then(layout) } }],
    ]) void window.__TAURI__?.event?.listen(name, listener).then(fn => { if (active) cleanups.push(fn); else fn() })
    if (artifacts) void refresh()
    else void layout().then(() => { if (active) return action('status') })
    return () => {
      active = false
      observer?.disconnect()
      window.removeEventListener('resize', resized)
      cleanups.forEach(fn => fn())
      void layout()
    }
  })
</script>
<section class="browser-workspace" aria-label={artifacts ? 'Artifacts' : 'Browser'}>
  {#if artifacts}
    <header class="artifact-tools">
      <h2>Artifacts</h2>
      {#if items.length}<select aria-label="Session artifacts" bind:value={selected}>{#each items as item}<option value={item.id}>{item.name}</option>{/each}</select>{/if}
      <button type="button" aria-label="Refresh artifacts" onclick={refresh}><LucideIcon name="refresh-cw" /></button>
    </header>
    {#if loading}<p role="status">Loading artifacts…</p>{:else if !items.length}<p class="empty">Artifacts created in chat appear here.</p>{/if}
  {:else}
    <form class="address" onsubmit={navigate}>
      <button type="button" aria-label="Back" onclick={() => action('back')}><LucideIcon name="arrow-left" /></button>
      <button type="button" aria-label="Forward" onclick={() => action('forward')}><LucideIcon name="arrow-right" /></button>
      <button type="button" aria-label="Reload" onclick={() => action('reload')}><LucideIcon name="refresh-cw" /></button>
      <button type="button" aria-label="Home" onclick={() => action('navigate', homepage)}><LucideIcon name="house" /></button>
      <input aria-label="Website address" placeholder="Enter a URL" bind:value={address} spellcheck="false" />
      <button type="button" aria-label="Browser settings" aria-expanded={settingsOpen} onclick={() => { homepageDraft = homepage; settingsError = ''; settingsOpen = !settingsOpen }}><LucideIcon name="settings" /></button>
    </form>
  {/if}
  {#if settingsOpen}
    <form class="browser-settings" onsubmit={saveSettings}>
      <header><h2>Browser settings</h2><button type="button" aria-label="Close browser settings" onclick={() => settingsOpen = false}><LucideIcon name="x" /></button></header>
      <label for="browser-homepage">Homepage</label>
      <input id="browser-homepage" bind:value={homepageDraft} placeholder={DEFAULT_HOMEPAGE} />
      <div class="settings-actions"><button type="submit">Save</button><button type="button" onclick={() => homepageDraft = DEFAULT_HOMEPAGE}>Use Muniment</button></div>
      {#if settingsError}<p role="alert">{settingsError}</p>{/if}
    </form>
  {/if}
  {#if status}<p role="status">{status}</p>{/if}
  <div class:hidden={artifacts && !selected} class="browser-host" bind:this={host} aria-label="Web page"></div>
</section>
<style>
  .browser-settings { display: grid; gap: 12px; padding: 16px; }
  .browser-settings header, .settings-actions { display: flex; align-items: center; gap: 8px; }
  .browser-settings header { justify-content: space-between; }
  .settings-actions button { width: auto; padding: 4px 10px; }
  .browser-workspace { flex:1; display:flex; flex-direction:column; min-width:0; min-height:0; overflow:hidden; background:var(--surface); }
  .address,.artifact-tools { display:flex; align-items:center; gap:4px; min-height:42px; padding:6px 8px; border-bottom:1px solid var(--border); }
  h2 { margin:0 8px 0 0; font-size:var(--text-13); }
  input,select { min-width:0; background:var(--faint); color:var(--ink); border:1px solid var(--border); border-radius:var(--radius-control); padding:4px 8px; font:var(--text-13) var(--font-human); height:28px; }
  button { display:flex; align-items:center; justify-content:center; width:28px; height:28px; padding:4px; background:transparent; color:var(--muted); border:0; border-radius:var(--radius-control); cursor:pointer; }
  button:hover { background:var(--faint); color:var(--ink); }
  .address input { flex:1; margin-left:4px; } select { flex:1; }
  .browser-host { flex:1; min-height:0; } .hidden { display:none; }
  p { margin:12px; color:var(--muted); font:var(--text-13) var(--font-human); }
  .empty { margin:auto; padding:24px; }
</style>

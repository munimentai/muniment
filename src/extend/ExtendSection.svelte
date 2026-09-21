<script>
  import SettingsTabs from '../lib/SettingsTabs.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import { onMount, tick } from 'svelte'
  import { catalog, categories, categoryLabel, filterCatalog, sortCatalog } from './catalog.js'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import McpIcon from './McpIcon.svelte'
  import ProviderIcon from './ProviderIcon.svelte'
  let { tauri, oncreate } = $props()
  let state = $state({ items: [], threads: {} }), tab = $state('mcp'), query = $state(''), category = $state(''), scope = $state('all'), page = $state(0), sort = $state('popular')
  let filtersOpen = $state(false), formDialog = $state()
  $effect(() => { if (form && formDialog && !formDialog.open) formDialog.showModal() })
  let details = $state(null), detailsDialog
  async function showDetails(entry) { details = entry; await tick(); detailsDialog.showModal() }
  let busy = $state(false), error = $state(''), status = $state(''), form = $state(null), preview = $state(null), selected = $state([])
  let name = $state(''), url = $state(''), source = $state(''), config = $state(''), token = $state(''), authentication = $state('none'), replaceId = $state(null)
  const mcpEntries = $derived([...catalog, ...state.items.filter(item => item.kind === 'mcp' && !catalog.some(entry => entry.source === item.source)).map(item => ({ ...item, source: item.source || `custom:${item.id}`, url: item.definition.url, categories: ['other'], popularity: 0 }))])
  const detailsInstalled = $derived(details && state.items.find(item => item.kind === 'mcp' && (item.source ? item.source === details.source : `custom:${item.id}` === details.source)))
  const counts = $derived({ mcp: mcpEntries.length, skill: state.items.filter(item => item.kind === 'skill').length, plugin: state.items.filter(item => item.kind === 'plugin').length })
  const installedSources = $derived(new Set(state.items.map(item => item.source || `custom:${item.id}`)))
  const filtered = $derived(sortCatalog(filterCatalog(mcpEntries, { query, category, installed: scope === 'installed', setup: scope === 'setup' }, installedSources), sort))
  const featured = $derived(!query.trim() && !category && scope === 'all' && sort === 'popular' ? filtered.slice(0, 12) : [])
  const remaining = $derived(featured.length ? filtered.slice(12) : filtered)
  const results = $derived(remaining.slice(page * 30, (page + 1) * 30))
  const unchanged = $derived(!!preview && !!replaceId && preview.digest === state.items.find(item => item.id === replaceId)?.digest)
  const installed = $derived(state.items.filter(item => item.kind === tab && `${item.name} ${item.description || ''}`.toLowerCase().includes(query.toLowerCase())))
  async function call(action, data = {}) { return tauri.invoke('extend_command', { action, data }) }
  async function work(fn) { busy = true; error = ''; status = ''; try { await fn() } catch (e) { error = typeof e === 'string' ? e : e.message || 'The operation failed.' } finally { busy = false } }
  async function refresh() { const result = await call('read'); if (result?.items) state = result }
  onMount(() => { void work(refresh) })
  function resetSearch() { query = ''; category = ''; scope = 'all'; sort = 'popular'; page = 0 }
  function add(entry = null) { form = entry ? { ...entry, id: entry.kind === 'mcp' ? entry.id : undefined } : {}; name = entry?.name || ''; url = entry?.url || ''; source = entry?.source || ''; config = ''; token = ''; authentication = entry?.url && entry?.kind !== 'mcp' ? 'oauth' : 'none'; preview = null; error = ''; replaceId = null }
  async function connectCatalog(entry) {
    await work(async () => {
      if (!entry.url) throw new Error('This provider has not published a connection URL. Use Custom with the URL supplied by the provider.')
      status = 'Opening browser sign-in…'
      state = await call('server', { name: entry.name, source: entry.source, description: entry.description || '', definition: { url: entry.url, auth: 'oauth' } })
      const item = state.items.find(item => item.source === entry.source)
      if (!item) throw new Error('The server could not be saved.')
      status = 'Complete sign-in in your browser.'
      const result = await call('auth', { id: item.id })
      status = `${item.name}: ${result.status}. ${result.tools.length} tools available.`
      await refresh()
    })
  }
  async function saveServer() {
    await work(async () => {
      const definition = config.trim() ? JSON.parse(config) : { url: url.trim(), ...(authentication === 'oauth' ? { auth: 'oauth' } : {}) }
      const savedId = form?.id
      state = await call('server', { id: savedId, name, source, definition, token, fetchIcon: !form?.source, description: form?.description || form?.categories?.map(categoryLabel).join(', ') || '' })
      form = null; token = ''; status = 'Server saved. Test the connection or sign in.'
      if (authentication === 'oauth') {
        const item = savedId ? state.items.find(item => item.id === savedId) : state.items.at(-1)
        status = 'Complete sign-in in your browser.'
        const result = await call('auth', { id: item.id })
        status = `${item.name}: ${result.status}. ${result.tools.length} tools available.`
        await refresh()
      }
    })
  }
  async function inspect() { await work(async () => { preview = await call('preview', { source: source.trim(), kind: tab }); selected = preview.skills.map(skill => skill.path) }) }
  async function install() { await work(async () => { state = await call('install', { previewId: preview.id, skills: selected, replaceId }); preview = null; form = null; status = 'Installed. Use the composer to select it.' }) }
  async function mutate(action, data) { await work(async () => { state = await call(action, data) }) }
  async function connection(item, action) { await work(async () => { const result = await call(action, { id: item.id }); status = `${item.name}: ${result.status}. ${result.tools.length} tools available.`; await refresh() }) }
  function update(item) { form = {}; source = item.source; replaceId = item.id; preview = null; error = '' }
</script>
<section class="extend" aria-label="Extend">
  <p class="intro">Add tools and instructions to your local workspace.</p>
  <SettingsTabs label="Extension types" value={tab} tabs={[{id:'mcp',label:'MCP servers',icon:'mcp',count:counts.mcp},{id:'skill',label:'Skills',icon:'pencil-sparkles',count:counts.skill},{id:'plugin',label:'Plugins',icon:'unplug',count:counts.plugin}]} onchange={id => { tab = id; resetSearch(); form = null; preview = null }} />
  <div class="toolbar">{#if tab === 'mcp'}<button type="button" class="filter-toggle" aria-label="Filters" aria-expanded={filtersOpen} aria-controls="mcp-filters" onclick={() => filtersOpen = !filtersOpen}><LucideIcon name="sliders-vertical" size={18} /></button>{/if}{#if tab === 'mcp' || counts[tab]}<input type="search" aria-label={`Search ${tab === 'mcp' ? 'MCP servers' : tab === 'skill' ? 'skills' : 'plugins'}`} placeholder="Search names, descriptions, or categories" bind:value={query} oninput={() => page = 0} />{/if}<button type="button" disabled={busy} onclick={() => add()}>{tab === 'mcp' ? 'Custom' : 'Install'}</button>{#if tab !== 'mcp'}{#if oncreate}<button type="button" onclick={() => oncreate(tab)}>Create from chat</button>{/if}<button type="button" disabled={busy} onclick={() => work(() => call('open_folder'))}>Open folder</button>{/if}</div>
  {#if tab === 'mcp' && filtersOpen}
    <div class="filters" id="mcp-filters">
      <label>Category<select bind:value={category} onchange={() => page = 0}><option value="">All categories</option>{#each categories as value}<option value={value}>{categoryLabel(value)}</option>{/each}</select></label>
      <label>Show<select bind:value={scope} onchange={() => page = 0}><option value="all">All servers</option><option value="installed">Installed</option><option value="setup">Setup required</option></select></label>
      <label>Sort<select bind:value={sort} onchange={() => page = 0}><option value="popular">Popularity</option><option value="name">Name A–Z</option><option value="name-desc">Name Z–A</option></select></label>
      <button type="button" onclick={resetSearch}>Clear filters</button>
    </div>
  {/if}
  {#if busy}<p role="status">{form && preview ? 'Installing package…' : 'Working…'}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
  {#if form}
    <dialog class="form server-details" data-panel="extension-setup" data-panel-variant="overlay" bind:this={formDialog} oncancel={event => { if (busy) event.preventDefault(); else { form = null; preview = null; token = '' } }} aria-label={tab === 'mcp' ? 'Add MCP server' : 'Add extension source'}>
      <div class="head"><h4>{tab === 'mcp' ? name || 'Add MCP server' : replaceId ? 'Review update' : 'Add source'}</h4><button type="button" disabled={busy} onclick={() => { form = null; preview = null; token = '' }}>Cancel</button></div>
      {#if tab === 'mcp'}
        {#if form.source && !form.url}<p>This entry needs a local command or a URL from your provider.</p>{#if form.website}<a href={form.website} target="_blank" rel="noreferrer">View setup instructions</a>{/if}{/if}
        <label>Name<input bind:value={name} /></label><label>Server URL<input type="url" bind:value={url} placeholder="https://example.com/mcp" /></label>
        <label>Authentication<select bind:value={authentication}><option value="none">None or token</option><option value="oauth">Sign in with OAuth</option></select></label>
        <label>Bearer token<input type="password" bind:value={token} autocomplete="off" placeholder="Optional. Stored in the system credential store." /></label>
        <details><summary>Local command or advanced configuration</summary><p>Use a command and argument list for local servers. Use environment variable references for secrets.</p><textarea aria-label="Server configuration" bind:value={config} rows="5" placeholder={'{"command":"npx","args":["-y","server-package"]}'}></textarea></details>
        <button type="button" disabled={busy || !name.trim() || (!url.trim() && !config.trim())} onclick={saveServer}>{authentication === 'oauth' ? 'Save and sign in' : 'Save server'}</button>
      {:else}
        <label>GitHub repository, local folder, or archive<input bind:value={source} placeholder="https://github.com/owner/repository" /></label>
        <p>Use a GitHub URL, a local folder, or a ZIP or TAR archive. GitHub URLs can include a branch and subfolder. Files stay pinned until you install an update.</p>
        {#if !preview}<button type="button" disabled={busy || !source.trim()} onclick={inspect}>Review package</button>{/if}
        {#if preview}
          <h4>{preview.name}</h4><p>{preview.description}</p><p>Version {preview.version.slice(0, 12)}</p>
          {#each preview.skills as skill}<label class="check"><input type="checkbox" value={skill.path} bind:group={selected} />{skill.name}<span>{skill.description}</span></label>{/each}
          {#if Object.keys(preview.dependencies || {}).length}<p>Package dependencies: {Object.keys(preview.dependencies).join(', ')}. Install scripts stay off.</p>{/if}
          {#if preview.extensions.length}<p>This plugin runs {preview.extensions.length} code extensions when invoked.</p>{/if}
          {#if Object.keys(preview.servers).length}<p>Included MCP servers: {Object.keys(preview.servers).join(', ')}</p>{/if}
          {#if unchanged}<p>This matches the installed version.</p>{/if}
          <button type="button" disabled={busy || unchanged} onclick={install}>{replaceId ? 'Install update' : 'Install selected'}</button>
        {/if}
      {/if}
      {#if busy}<p role="status">Working…</p>{/if}
      {#if error}<p role="alert">{error}</p>{/if}
    </dialog>
  {/if}
  {#if tab !== 'mcp' && installed.length}
    <h4>Installed</h4>
    <div class="list" aria-label="Installed extensions">{#each installed as item (item.id)}<article class="entry">
      <div class="head"><strong>{#if item.kind === 'mcp'}<ProviderIcon entry={item} size={24} />{/if}{item.name}</strong><Toggle checked={item.enabled !== false} label={`Use ${item.name} in chats`} disabled={busy} onchange={enabled => mutate('toggle', { id: item.id, enabled })} /></div>
      <p>{item.description || (item.kind === 'mcp' ? item.definition.url || item.definition.command : item.source)}</p>
      {#if item.lastCheck}<p>Last connection test: {item.lastCheck.status}. {item.lastCheck.tools} tools.</p>{/if}
      {#if item.kind === 'plugin'}{#each Object.entries(item.servers || {}) as [serverName, definition]}<div class="actions"><McpIcon /><span>{serverName}</span><button type="button" disabled={busy} onclick={() => connection({id: `${item.id}:${serverName}`,name: serverName}, 'test')}>Test connection</button>{#if definition.url}<button type="button" disabled={busy} onclick={() => connection({id: `${item.id}:${serverName}`,name: serverName}, 'auth')}>Sign in</button>{/if}</div>{/each}{/if}
      <div class="actions">{#if item.kind === 'mcp'}<button type="button" disabled={busy} onclick={() => connection(item, 'test')}>Test connection</button>{#if item.definition.url}<button type="button" disabled={busy} onclick={() => connection(item, 'auth')}>Sign in</button>{/if}<button type="button" disabled={busy} onclick={() => { add(item); config = JSON.stringify(item.definition, null, 2) }}>Configure</button>{:else}<button type="button" disabled={busy} onclick={() => work(() => call('open_folder', { id: item.id }))}>Open folder</button><button type="button" disabled={busy} onclick={() => update(item)}>Check for update</button>{#if item.previous}<button type="button" disabled={busy} onclick={() => mutate('rollback', { id: item.id })}>Restore previous version</button>{/if}{/if}<button type="button" disabled={busy} onclick={() => mutate('remove', { id: item.id })}>Uninstall</button></div>
    </article>{/each}</div>
  {:else if tab !== 'mcp'}<p>No {tab === 'skill' ? 'skills' : 'plugins'} installed. Add a GitHub repository, local folder, or archive to get started.</p>{/if}
  {#if tab === 'mcp'}
    {#snippet cards(entries, label)}
      <div class="catalog-grid" aria-label={label}>{#each entries as entry (entry.id)}{@const connected = state.items.find(item => item.kind === 'mcp' && (item.source ? item.source === entry.source : `custom:${item.id}` === entry.source))}<article class="entry catalog-card" class:connected={!!connected}>
        <div class="card-title"><ProviderIcon {entry} /><strong>{entry.name}</strong></div>
        {#if entry.publisher}<p class="publisher">{entry.publisher}</p>{/if}
        <p class="category">{entry.categories.map(categoryLabel).join(' · ')}</p>
        <div class="actions"><button type="button" class="details-link" onclick={() => showDetails(entry)}>Details</button>{#if connected}<Toggle checked={connected.enabled !== false} label={`Use ${entry.name} in chats`} disabled={busy} onchange={enabled => mutate('toggle', { id: connected.id, enabled })} />{:else}<button type="button" disabled={busy} onclick={() => connectCatalog(entry)}>Connect</button>{/if}</div>
      </article>{/each}</div>
    {/snippet}
    {#if featured.length}
      <h4>Popular MCPs</h4>
      {@render cards(featured, 'Popular MCP servers')}
      <h4>More MCPs</h4>
    {/if}
    {@render cards(results, 'MCP catalog')}
    {#if !filtered.length}<p>No servers match these filters.</p>{/if}
    {#if remaining.length > 30}<nav class="pages" aria-label="Catalog pages"><button type="button" disabled={page === 0} onclick={() => page--}>Previous</button><span>Page {page + 1} of {Math.ceil(remaining.length / 30)}</span><button type="button" disabled={(page + 1) * 30 >= remaining.length} onclick={() => page++}>Next</button></nav>{/if}
  {/if}
</section>
<dialog data-panel="mcp-details" data-panel-variant="overlay" class="server-details" bind:this={detailsDialog} aria-labelledby="server-details-title" onclose={() => details = null}>
  {#if details}
    <div class="head"><div class="card-title"><ProviderIcon entry={details} /><h2 id="server-details-title">{details.name}</h2></div><button type="button" onclick={() => detailsDialog.close()}>Close</button></div>
    <p class="description">{details.description || 'The provider has not supplied a description.'}</p>
    <dl>
      {#if details.publisher}<dt>Publisher</dt><dd>{details.publisher}</dd>{/if}
      <dt>Categories</dt><dd>{details.categories.map(categoryLabel).join(' · ')}</dd>
      {#if detailsInstalled?.definition.command}
        <dt>Connection</dt><dd>Local MCP server</dd>
        <dt>Command</dt><dd>{detailsInstalled.definition.command}</dd>
      {:else}
        <dt>Connection</dt><dd>Remote MCP server</dd>
        <dt>Server URL</dt><dd>{details.url || 'Get the server URL from the provider.'}</dd>
      {/if}
      {#if details.website}<dt>Provider website</dt><dd><a href={details.website} target="_blank" rel="noreferrer">{details.website}</a></dd>{/if}
    </dl>
    {#if detailsInstalled}
      {#if detailsInstalled.lastCheck}<p>Last connection test: {detailsInstalled.lastCheck.status}. {detailsInstalled.lastCheck.tools} tools.</p>{/if}
      <div class="detail-actions actions">
        <button type="button" disabled={busy} onclick={() => connection(detailsInstalled, 'test')}>Test connection</button>
        {#if detailsInstalled.definition.url}<button type="button" disabled={busy} onclick={() => connection(detailsInstalled, 'auth')}>Sign in</button>{/if}
        <button type="button" disabled={busy} onclick={() => { const item = detailsInstalled; detailsDialog.close(); add(item); config = JSON.stringify(item.definition, null, 2) }}>Configure</button>
        <button type="button" disabled={busy} onclick={() => { const id = detailsInstalled.id; detailsDialog.close(); void mutate('remove', { id }) }}>Uninstall</button>
      </div>
      {#if error}<p role="alert">{error}</p>{/if}
      {#if status}<p role="status">{status}</p>{/if}
    {:else}
      <p class="requirements">The provider may require an account or subscription. Configure authentication when you add the server.</p>
      <div class="detail-actions"><button type="button" disabled={busy} onclick={() => { const entry = details; detailsDialog.close(); void connectCatalog(entry) }}>Connect</button></div>
    {/if}
  {/if}
</dialog>

<style>
  .tab-count { font-size: var(--text-12); font-variant-numeric: tabular-nums; }
  .filter-toggle { display: grid; place-items: center; flex: none; }
  .filter-toggle[aria-expanded="true"] { color: var(--accent); }
  .server-details { position: fixed; inset: 0; margin: auto; width: min(560px, calc(100vw - 48px)); max-height: calc(100vh - 48px); overflow: auto; box-sizing: border-box; padding: 24px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--paper); color: var(--ink); font-family: var(--font-human); }
  .server-details::backdrop { background: var(--overlay-backdrop); }
  .server-details h2 { margin: 0; font-size: var(--text-22); overflow-wrap: anywhere; }
  .description { margin: 20px 0; line-height: var(--leading-body); overflow-wrap: anywhere; }
  .server-details dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 12px 20px; font-size: var(--text-13); }
  .server-details dt, .requirements { color: var(--muted); }
  .server-details dd { margin: 0; overflow-wrap: anywhere; }
  .server-details a { color: var(--ink); text-decoration: underline; }
  .requirements { margin-top: 20px; font-size: var(--text-13); line-height: var(--leading-body); }
  .detail-actions { display: flex; justify-content: flex-end; margin-top: 20px; }
  button.details-link { padding: 0; background: transparent; text-decoration: underline; text-underline-offset: 3px; }

  .extend { display: grid; gap: 14px; min-width: 0; } .intro { color: var(--muted); } p, h4 { margin: 0; } .toolbar, .filters, .actions, .pages, .head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; } .head { justify-content: space-between; }     .toolbar input { flex: 1; min-width: 150px; } label { display: grid; gap: 5px; } .filters label { flex: 1; min-width: 110px; color: var(--muted); } input, select, textarea { background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px; min-width: 0; } textarea { width: 100%; box-sizing: border-box; } .form { display: grid; gap: 12px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); } .list { display: grid; gap: 8px; } .entry { display: grid; gap: 8px; border: 1px solid var(--border); border-radius: var(--radius-panel); padding: 12px; } .entry p { color: var(--muted); overflow-wrap: anywhere; } .check { display: flex; align-items: center; gap: 7px; font-size: var(--text-12); } .check span { color: var(--muted); } .pages { justify-content: space-between; } button, a { font-size: var(--text-12); } [role=alert] { color: var(--ink); }
  .extend { container-type: inline-size; }
  button { font-family: var(--font-human); border: 0; border-radius: var(--radius-control); padding: 5px 9px; color: var(--ink); background: var(--faint); cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  .catalog-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }
  .catalog-card.connected { background: var(--signal-soft); border-color: var(--signal); }
  .catalog-card.connected strong { color: var(--signal); }
  .catalog-card { min-width: 0; gap: 5px; align-content: start; }
  .card-title { display: flex; align-items: center; gap: 9px; min-width: 0; }
  .card-title strong { display: block; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: var(--text-13); }
  .publisher { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--text-12); }
  .category { font-size: var(--text-provenance); min-height: 2.8em; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .catalog-card .actions { justify-content: space-between; gap: 8px; margin-top: 5px; }
  @container (max-width: 680px) { .catalog-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @container (max-width: 390px) { .catalog-grid { grid-template-columns: minmax(0, 1fr); } }
</style>

<script>
  import { onMount } from 'svelte'
  import { catalog, categories, categoryLabel, filterCatalog, sortCatalog } from './catalog.js'
  import McpIcon from './McpIcon.svelte'
  import ProviderIcon from './ProviderIcon.svelte'
  let { tauri } = $props()
  let state = $state({ items: [], threads: {} }), tab = $state('mcp'), query = $state(''), category = $state(''), type = $state(''), scope = $state('all'), page = $state(0), sort = $state('popular')
  let busy = $state(false), error = $state(''), status = $state(''), form = $state(null), preview = $state(null), selected = $state([])
  let name = $state(''), url = $state(''), source = $state(''), config = $state(''), token = $state(''), authentication = $state('none'), replaceId = $state(null)
  const installedSources = $derived(new Set(state.items.map(item => item.source)))
  const filtered = $derived(sortCatalog(filterCatalog(catalog, { query, category, type, installed: scope === 'installed', setup: scope === 'setup' }, installedSources), sort))
  const featured = $derived(!query.trim() && !category && !type && scope === 'all' && sort === 'popular' ? filtered.slice(0, 12) : [])
  const remaining = $derived(featured.length ? filtered.slice(12) : filtered)
  const results = $derived(remaining.slice(page * 30, (page + 1) * 30))
  const unchanged = $derived(!!preview && !!replaceId && preview.digest === state.items.find(item => item.id === replaceId)?.digest)
  const installed = $derived(state.items.filter(item => item.kind === tab && `${item.name} ${item.description || ''}`.toLowerCase().includes(query.toLowerCase())))
  async function call(action, data = {}) { return tauri.invoke('extend_command', { action, data }) }
  async function work(fn) { busy = true; error = ''; status = ''; try { await fn() } catch (e) { error = typeof e === 'string' ? e : e.message || 'The operation failed.' } finally { busy = false } }
  async function refresh() { const result = await call('read'); if (result?.items) state = result }
  onMount(() => { void work(refresh) })
  function resetSearch() { query = ''; category = ''; type = ''; scope = 'all'; sort = 'popular'; page = 0 }
  function add(entry = null) { form = entry || {}; name = entry?.name || ''; url = entry?.url || ''; source = entry?.source || ''; config = ''; token = ''; authentication = 'none'; preview = null; error = ''; replaceId = null }
  async function saveServer() {
    await work(async () => {
      const definition = config.trim() ? JSON.parse(config) : { url: url.trim(), ...(authentication === 'oauth' ? { auth: 'oauth' } : {}) }
      state = await call('server', { id: form?.id, name, source, definition, token, description: form?.categories?.map(categoryLabel).join(', ') || form?.description || '' }); form = null; token = ''; status = 'Server saved. Test the connection or sign in.'
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
  <div class="tabs" role="tablist" aria-label="Extension types">
    {#each [['mcp', 'MCP servers'], ['skill', 'Skills'], ['plugin', 'Plugins']] as [id, label]}<button type="button" role="tab" aria-selected={tab === id} onclick={() => { tab = id; resetSearch(); form = null; preview = null }}>{#if id === 'mcp'}<McpIcon />{/if}{label}</button>{/each}
  </div>
  <div class="toolbar"><input type="search" aria-label={`Search ${tab === 'mcp' ? 'MCP servers' : tab === 'skill' ? 'skills' : 'plugins'}`} placeholder="Search names, publishers, or categories" bind:value={query} oninput={() => page = 0} /><button type="button" disabled={busy} onclick={() => add()}>{tab === 'mcp' ? 'Add server' : 'Add source'}</button></div>
  {#if tab === 'mcp'}
    <div class="filters">
      <label>Category<select bind:value={category} onchange={() => page = 0}><option value="">All categories</option>{#each categories as value}<option value={value}>{categoryLabel(value)}</option>{/each}</select></label>
      <label>Connection<select bind:value={type} onchange={() => page = 0}><option value="">All connections</option><option value="remote">Remote</option><option value="local">Local</option></select></label>
      <label>Show<select bind:value={scope} onchange={() => page = 0}><option value="all">All servers</option><option value="installed">Installed</option><option value="setup">Setup required</option></select></label>
      <label>Sort<select bind:value={sort} onchange={() => page = 0}><option value="popular">Popularity</option><option value="name">Name A–Z</option><option value="name-desc">Name Z–A</option></select></label>
      <button type="button" onclick={resetSearch}>Clear filters</button>
    </div>
  {/if}
  {#if busy}<p role="status">{form && preview ? 'Installing package…' : 'Working…'}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
  {#if form}
    <section class="form" aria-label={tab === 'mcp' ? 'Add MCP server' : 'Add extension source'}>
      <div class="head"><h4>{tab === 'mcp' ? name || 'Add MCP server' : replaceId ? 'Review update' : 'Add source'}</h4><button type="button" disabled={busy} onclick={() => { form = null; preview = null; token = '' }}>Cancel</button></div>
      {#if tab === 'mcp'}
        {#if form.source && !form.url}<p>This entry needs a local command or a URL from your provider.</p><a href={form.source} target="_blank" rel="noreferrer">View setup instructions</a>{/if}
        <label>Name<input bind:value={name} /></label><label>Server URL<input type="url" bind:value={url} placeholder="https://example.com/mcp" /></label>
        <label>Authentication<select bind:value={authentication}><option value="none">None or token</option><option value="oauth">Sign in with OAuth</option></select></label>
        <label>Bearer token<input type="password" bind:value={token} autocomplete="off" placeholder="Optional. Stored in the system credential store." /></label>
        <details><summary>Local command or advanced configuration</summary><p>Use a command and argument list for local servers. Use environment variable references for secrets.</p><textarea aria-label="Server configuration" bind:value={config} rows="5" placeholder={'{"command":"npx","args":["-y","server-package"]}'}></textarea></details>
        <button type="button" disabled={busy || !name.trim() || (!url.trim() && !config.trim())} onclick={saveServer}>Save server</button>
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
    </section>
  {/if}
  {#if installed.length}
    <h4>Installed</h4>
    <div class="list" aria-label="Installed extensions">{#each installed as item (item.id)}<article class="entry">
      <div class="head"><strong>{#if item.kind === 'mcp'}<McpIcon />{/if}{item.name}</strong><label class="check"><input type="checkbox" checked={item.enabled !== false} disabled={busy} onchange={e => mutate('toggle', { id: item.id, enabled: e.currentTarget.checked })} />Available in chats</label></div>
      <p>{item.description || (item.kind === 'mcp' ? item.definition.url || item.definition.command : item.source)}</p>
      {#if item.lastCheck}<p>Last connection test: {item.lastCheck.status}. {item.lastCheck.tools} tools.</p>{/if}
      {#if item.kind === 'plugin'}{#each Object.entries(item.servers || {}) as [serverName, definition]}<div class="actions"><McpIcon /><span>{serverName}</span><button type="button" disabled={busy} onclick={() => connection({id: `${item.id}:${serverName}`,name: serverName}, 'test')}>Test connection</button>{#if definition.url}<button type="button" disabled={busy} onclick={() => connection({id: `${item.id}:${serverName}`,name: serverName}, 'auth')}>Sign in</button>{/if}</div>{/each}{/if}
      <div class="actions">{#if item.kind === 'mcp'}<button type="button" disabled={busy} onclick={() => connection(item, 'test')}>Test connection</button>{#if item.definition.url}<button type="button" disabled={busy} onclick={() => connection(item, 'auth')}>Sign in</button>{/if}<button type="button" disabled={busy} onclick={() => { add(item); config = JSON.stringify(item.definition, null, 2) }}>Configure</button>{:else}<button type="button" disabled={busy} onclick={() => update(item)}>Check for update</button>{#if item.previous}<button type="button" disabled={busy} onclick={() => mutate('rollback', { id: item.id })}>Restore previous version</button>{/if}{/if}<button type="button" disabled={busy} onclick={() => mutate('remove', { id: item.id })}>Uninstall</button></div>
    </article>{/each}</div>
  {:else if tab !== 'mcp'}<p>No {tab === 'skill' ? 'skills' : 'plugins'} installed. Add a GitHub repository, local folder, or archive to get started.</p>{/if}
  {#if tab === 'mcp'}
    <div class="head"><h4>Discover MCP servers</h4><span aria-live="polite">{filtered.length} {filtered.length === 1 ? 'result' : 'results'}</span></div>
    <p class="intro">Includes all {catalog.length} entries from Anthropic’s public catalog. Providers control access and account requirements.</p>
    {#snippet cards(entries, label)}
      <div class="catalog-grid" aria-label={label}>{#each entries as entry (entry.id)}<article class="entry catalog-card">
        <div class="card-title"><ProviderIcon {entry} /><strong>{entry.name}</strong></div>
        <p class="publisher">{entry.publisher}</p>
        <p class="category">{entry.categories.map(categoryLabel).join(' · ')}</p>
        <div class="actions"><span class="connection">{entry.type === 'local' ? 'Local' : 'Remote'}</span><a href={entry.source} target="_blank" rel="noreferrer">Details</a><button type="button" disabled={busy || installedSources.has(entry.source)} onclick={() => add(entry)}>{installedSources.has(entry.source) ? 'Installed' : entry.url ? 'Add' : 'Set up'}</button></div>
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
<style>
  .extend { display: grid; gap: 14px; min-width: 0; } .intro { color: var(--muted); } p, h4 { margin: 0; } .tabs, .toolbar, .filters, .actions, .pages, .head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; } .head { justify-content: space-between; } .tabs button, strong { display: inline-flex; align-items: center; gap: 8px; } .tabs button { border: 0; border-radius: var(--radius-pill); padding: 6px 12px; background: transparent; color: var(--muted); font: inherit; font-size: var(--text-13); cursor: pointer; } .tabs button[aria-selected=true] { background: var(--signal-soft); color: var(--signal); } .tabs button:hover { background: var(--faint); } .toolbar input { flex: 1; min-width: 150px; } label { display: grid; gap: 5px; } .filters label { flex: 1; min-width: 110px; color: var(--muted); } input, select, textarea { background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px; min-width: 0; } textarea { width: 100%; box-sizing: border-box; } .form { display: grid; gap: 12px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); } .list { display: grid; gap: 8px; } .entry { display: grid; gap: 8px; border: 1px solid var(--border); border-radius: var(--radius-panel); padding: 12px; } .entry p { color: var(--muted); overflow-wrap: anywhere; } .check { display: flex; align-items: center; gap: 7px; font-size: var(--text-12); } .check span { color: var(--muted); } .pages { justify-content: space-between; } button, a { font-size: var(--text-12); } [role=alert] { color: var(--ink); }
  .extend { container-type: inline-size; }
  button { font-family: var(--font-human); border: 0; border-radius: var(--radius-control); padding: 5px 9px; color: var(--ink); background: var(--faint); cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  .catalog-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }
  .catalog-card { min-width: 0; gap: 5px; align-content: start; }
  .card-title { display: flex; align-items: center; gap: 9px; min-width: 0; }
  .card-title strong { display: block; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: var(--text-13); }
  .publisher { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--text-12); }
  .category { font-size: var(--text-provenance); min-height: 2.8em; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .connection { margin-right: auto; color: var(--muted); font-size: var(--text-provenance); }
  .catalog-card .actions { gap: 8px; margin-top: 5px; }
  @container (max-width: 680px) { .catalog-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @container (max-width: 390px) { .catalog-grid { grid-template-columns: minmax(0, 1fr); } }
</style>

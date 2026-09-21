<script>
  import { onMount } from 'svelte'
  import { catalog, categories, categoryLabel, filterCatalog } from './catalog.js'
  import McpIcon from './McpIcon.svelte'
  let { tauri } = $props()
  let state = $state({ items: [], threads: {} }), tab = $state('mcp'), query = $state(''), category = $state(''), type = $state(''), scope = $state('all'), page = $state(0)
  let busy = $state(false), error = $state(''), status = $state(''), form = $state(null), preview = $state(null), selected = $state([])
  let name = $state(''), url = $state(''), source = $state(''), config = $state(''), token = $state(''), authentication = $state('none'), replaceId = $state(null)
  const installedSources = $derived(new Set(state.items.map(item => item.source)))
  const filtered = $derived(filterCatalog(catalog, { query, category, type, installed: scope === 'installed', setup: scope === 'setup' }, installedSources))
  const results = $derived(filtered.slice(page * 30, (page + 1) * 30))
  const installed = $derived(state.items.filter(item => item.kind === tab && `${item.name} ${item.description || ''}`.toLowerCase().includes(query.toLowerCase())))
  async function call(action, data = {}) { return tauri.invoke('extend_command', { action, data }) }
  async function work(fn) { busy = true; error = ''; status = ''; try { await fn() } catch (e) { error = typeof e === 'string' ? e : e.message || 'The operation failed.' } finally { busy = false } }
  async function refresh() { const result = await call('read'); if (result?.items) state = result }
  onMount(() => { void work(refresh) })
  function resetSearch() { query = ''; category = ''; type = ''; scope = 'all'; page = 0 }
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
          <button type="button" disabled={busy} onclick={install}>{replaceId ? 'Install update' : 'Install selected'}</button>
        {/if}
      {/if}
    </section>
  {/if}
  {#if installed.length}
    <h4>Installed</h4>
    <div class="list" aria-label="Installed extensions">{#each installed as item (item.id)}<article class="entry">
      <div class="head"><strong>{#if item.kind === 'mcp'}<McpIcon />{/if}{item.name}</strong><label class="check"><input type="checkbox" checked={item.enabled !== false} disabled={busy} onchange={e => mutate('toggle', { id: item.id, enabled: e.currentTarget.checked })} />Available in new chats</label></div>
      <p>{item.description || (item.kind === 'mcp' ? item.definition.url || item.definition.command : item.source)}</p>
      {#if item.lastCheck}<p>Last connection test: {item.lastCheck.status}. {item.lastCheck.tools} tools.</p>{/if}
      {#if item.kind === 'plugin'}{#each Object.entries(item.servers || {}) as [serverName, definition]}<div class="actions"><McpIcon /><span>{serverName}</span><button type="button" disabled={busy} onclick={() => connection({id: `${item.id}:${serverName}`,name: serverName}, 'test')}>Test connection</button>{#if definition.url}<button type="button" disabled={busy} onclick={() => connection({id: `${item.id}:${serverName}`,name: serverName}, 'auth')}>Sign in</button>{/if}</div>{/each}{/if}
      <div class="actions">{#if item.kind === 'mcp'}<button type="button" disabled={busy} onclick={() => connection(item, 'test')}>Test connection</button>{#if item.definition.url}<button type="button" disabled={busy} onclick={() => connection(item, 'auth')}>Sign in</button>{/if}<button type="button" disabled={busy} onclick={() => { add(item); config = JSON.stringify(item.definition, null, 2) }}>Configure</button>{:else}<button type="button" disabled={busy} onclick={() => update(item)}>Check for update</button>{#if item.previous}<button type="button" disabled={busy} onclick={() => mutate('rollback', { id: item.id })}>Restore previous version</button>{/if}{/if}<button type="button" disabled={busy} onclick={() => mutate('remove', { id: item.id })}>Uninstall</button></div>
    </article>{/each}</div>
  {:else if tab !== 'mcp'}<p>No {tab === 'skill' ? 'skills' : 'plugins'} installed. Add a GitHub repository, local folder, or archive to get started.</p>{/if}
  {#if tab === 'mcp'}
    <div class="head"><h4>Discover MCP servers</h4><span aria-live="polite">{filtered.length} {filtered.length === 1 ? 'result' : 'results'}</span></div>
    <p class="intro">Includes all {catalog.length} entries from Anthropic’s public catalog. Providers control access and account requirements.</p>
    <div class="list" aria-label="MCP catalog">{#each results as entry (entry.id)}<article class="entry">
      <div class="head"><strong><McpIcon />{entry.name}</strong><span>{entry.type === 'local' ? 'Local' : 'Remote'}</span></div>
      <p>{entry.publisher} · {entry.categories.map(categoryLabel).join(' · ')}</p>
      <div class="actions"><a href={entry.source} target="_blank" rel="noreferrer">Details</a><button type="button" disabled={busy || installedSources.has(entry.source)} onclick={() => add(entry)}>{installedSources.has(entry.source) ? 'Installed' : entry.url ? 'Add server' : 'Set up'}</button></div>
    </article>{/each}</div>
    {#if !results.length}<p>No servers match these filters.</p>{/if}
    <nav class="pages" aria-label="Catalog pages"><button type="button" disabled={page === 0} onclick={() => page--}>Previous</button><span>Page {page + 1} of {Math.max(1, Math.ceil(filtered.length / 30))}</span><button type="button" disabled={(page + 1) * 30 >= filtered.length} onclick={() => page++}>Next</button></nav>
    <small>MCP icon: Microsoft Codicons, <a href="https://creativecommons.org/licenses/by/4.0/" target="_blank" rel="noreferrer">CC BY 4.0</a>. <a href="https://github.com/microsoft/vscode-codicons" target="_blank" rel="noreferrer">Source</a></small>
  {/if}
</section>
<style>
  .extend { display: grid; gap: 14px; min-width: 0; } .intro, small { color: var(--muted); } p, h4 { margin: 0; } .tabs, .toolbar, .filters, .actions, .pages, .head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; } .head { justify-content: space-between; } .tabs button, strong { display: inline-flex; align-items: center; gap: 8px; } .tabs [aria-selected=true] { background: var(--surface-2, var(--border)); color: var(--ink); } .toolbar input { flex: 1; min-width: 150px; } label { display: grid; gap: 5px; } .filters label { flex: 1; min-width: 110px; color: var(--muted); } input, select, textarea { background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px; min-width: 0; } textarea { width: 100%; box-sizing: border-box; } .form { display: grid; gap: 12px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); } .list { display: grid; gap: 8px; } .entry { display: grid; gap: 8px; border: 1px solid var(--border); border-radius: var(--radius-panel); padding: 12px; } .entry p { color: var(--muted); overflow-wrap: anywhere; } .check { display: flex; align-items: center; gap: 7px; font-size: var(--text-12); } .check span { color: var(--muted); } .pages { justify-content: space-between; } button, a { font-size: var(--text-12); } [role=alert] { color: var(--ink); }
</style>

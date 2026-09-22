<script>
  import { panelScroll } from './panel-scroll.js'
  import CatalogActions from './CatalogActions.svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { selected = $bindable(null), projects = [], threads = [], assignments = {}, busy = false, error = '', loading = false, oncreate, onthread, onnewthread, onrename, onopen, onclose } = $props()
  let creating = $state(false)
  let name = $state('')
  let search = $state('')
  const project = $derived(projects.find(([id]) => id === selected))
  const matches = $derived(projects.filter(([, title]) => title.toLowerCase().includes(search.toLowerCase())))
  const projectThreads = $derived(threads.filter(thread => assignments[thread.threadId] === selected))
  async function create(event) {
    event.preventDefault()
    if (await oncreate(name.trim())) { creating = false; name = '' }
  }
</script>
<section data-panel="projects" use:panelScroll class="panel" aria-label="Projects catalog">
  <header data-panel-header>
    {#if project}<button aria-label="Back to projects" onclick={() => selected = null}><LucideIcon name="arrow-left" /></button>{/if}
    <h2>{project ? project[1] : 'Projects'}</h2>
    {#if project}<button disabled={busy} onclick={() => onnewthread(selected)}><LucideIcon name="square-pen" />New thread</button>
    {:else}<button disabled={busy} onclick={() => creating = true}><LucideIcon name="square-pen" />New project</button>{/if}
    <button aria-label="Close projects" onclick={onclose}><LucideIcon name="x" /></button>
  </header>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if creating}
    <form onsubmit={create}>
      <input aria-label="Project name" placeholder="Project name" maxlength="80" bind:value={name} disabled={busy} />
      <button disabled={busy || !name.trim()}>Create</button>
      <button type="button" disabled={busy} onclick={() => creating = false}>Cancel</button>
    </form>
  {/if}
  {#if project}
    <div class="catalog" aria-label="Project threads">
      {#each projectThreads as thread (thread.threadId)}<button class="card" disabled={busy} onclick={() => onthread(thread.threadId)}><LucideIcon name="file-text" size={24}/><strong>{thread.title || 'Untitled thread'}</strong></button>{/each}
    </div>
    {#if !projectThreads.length}<p>{loading ? 'Loading threads…' : 'No threads yet. Start a new thread in this project.'}</p>{/if}
  {:else}
    <input type="search" aria-label="Search projects" placeholder="Search projects" bind:value={search} />
    <div class="catalog">
      {#each matches as [id, title] (id)}
        {@const count = threads.filter(thread => assignments[thread.threadId] === id).length}
        <div class="card-wrap">
          <CatalogActions name={title} disabled={busy} allowArchive={false} allowDelete={false} maxNameLength={80} extraActions={[{id:'new',label:'New thread',icon:'square-pen'},{id:'open',label:'Open folder',icon:'folder-open'}]} onaction={(action, name) => action === 'rename' ? onrename(id, name) : action === 'new' ? onnewthread(id) : onopen(id)}/>
          <button class="card" aria-label={`${title} ${count} ${count === 1 ? 'thread' : 'threads'}`} disabled={busy} onclick={() => selected = id}><LucideIcon name="folder" size={32}/><strong>{title}</strong><span>{count} {count === 1 ? 'thread' : 'threads'}</span></button>
        </div>
      {/each}
    </div>
    {#if !matches.length}<p>{search ? 'No matching projects.' : 'Create a project to keep related threads together.'}</p>{/if}
  {/if}
</section>
<style>
  .panel { grid-area: thread; overflow: auto; padding: 0; }
  header, form { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
  h2 { flex: 1; font-size: var(--text-22); }
  button { display: inline-flex; align-items: center; gap: 8px; background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px 12px; cursor: pointer; font: var(--text-13) var(--font-human); }
  button:hover:not(:disabled) { background: var(--faint); }
  input { min-width: 0; flex: 1; margin-bottom: 12px; }
  form input { margin-bottom: 0; }
  .panel > :global(:not(header):not(.catalog)) {margin-left:12px;margin-right:12px;}
  .catalog {padding:0 12px 12px; display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }
  .card-wrap { position:relative; display:flex; }
  .card-wrap :global(.catalog-actions) { position:absolute; right:8px; top:8px; }
  .card { width:100%; flex-direction: column; align-items: flex-start; gap: 12px; text-align: left; padding: 20px; overflow-wrap: anywhere; }
  span, p { color: var(--muted); }
</style>

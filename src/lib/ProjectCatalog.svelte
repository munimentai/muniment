<script>
  // The Projects catalog: this computer's projects, then the projects other
  // fleet members own (`remote`, as `[id, name, owner]`), each with its
  // owner's chip. A project another member owns opens its threads and starts
  // new ones like its sidebar row, with no rename or folder actions.
  import PanelPage from './ui/PanelPage.svelte'
  import PageHead from './ui/PageHead.svelte'
  import CardGrid from './ui/CardGrid.svelte'
  import CatalogCard from './ui/CatalogCard.svelte'
  import SearchToolbar from './ui/SearchToolbar.svelte'
  import Button from './ui/Button.svelte'
  import CatalogActions from './CatalogActions.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import OwnerChip from '../runtimes/OwnerChip.svelte'
  let { selected = $bindable(null), projects = [], remote = [], threads = [], assignments = {}, busy = false, error = '', loading = false, oncreate, onthread, onnewthread, onrename, onopen } = $props()
  let creating = $state(false)
  let name = $state('')
  let search = $state('')
  const project = $derived(projects.find(([id]) => id === selected) ?? remote.find(([id]) => id === selected))
  const found = (rows) => rows.filter(([, title]) => title.toLowerCase().includes(search.toLowerCase()))
  const matches = $derived(found(projects))
  const remoteMatches = $derived(found(remote))
  const count = (id) => threads.filter(thread => assignments[thread.threadId] === id).length
  const projectThreads = $derived(threads.filter(thread => assignments[thread.threadId] === selected))
  async function create(event) {
    event.preventDefault()
    if (await oncreate(name.trim())) { creating = false; name = '' }
  }
</script>
<section data-panel="projects" class="panel" aria-label="Projects catalog">
  <h2 class="visually-hidden">{project ? project[1] : 'Projects'}</h2>
  <PanelPage>
    <PageHead intro={project ? `Threads in ${project[1]}.` : 'Projects group related threads, and each reply recalls context from the others.'}>
      {#snippet lead()}{#if project}<button type="button" class="back" aria-label="Back to projects" onclick={() => selected = null}><LucideIcon name="arrow-left" /></button>{/if}{/snippet}
      {#snippet actions()}
        {#if project}{#if project[2]}<OwnerChip machine={project[2]} prefix="files on" />{/if}<Button icon="square-pen" disabled={busy} onclick={() => onnewthread(selected)}>New thread</Button>
        {:else}<Button icon="plus" disabled={busy} onclick={() => creating = true}>New project</Button>{/if}
      {/snippet}
    </PageHead>
    {#if error}<p role="alert">{error}</p>{/if}
    {#if creating}
      <form onsubmit={create}>
        <input aria-label="Project name" placeholder="Project name" maxlength="80" bind:value={name} disabled={busy} />
        <Button type="submit" disabled={busy || !name.trim()}>Create</Button>
        <Button disabled={busy} onclick={() => creating = false}>Cancel</Button>
      </form>
    {/if}
    {#if project}
      <CardGrid aria-label="Project threads">
        {#each projectThreads as thread (thread.threadId)}<CatalogCard disabled={busy} onclick={() => onthread(thread.threadId)}><LucideIcon name="file-text" size={24}/><strong>{thread.title || 'Untitled thread'}</strong></CatalogCard>{/each}
      </CardGrid>
      {#if !projectThreads.length}<p>{loading ? 'Loading threads…' : 'No threads yet. Start a new thread in this project.'}</p>{/if}
    {:else}
      {#if projects.length + remote.length}<SearchToolbar label="Search projects" placeholder="Search projects" bind:value={search} />{/if}
      <CardGrid>
        {#each matches as [id, title] (id)}
          {@const total = count(id)}
          <CatalogCard aria-label={`${title} ${total} ${total === 1 ? 'thread' : 'threads'}`} disabled={busy} onclick={() => selected = id}>
            {#snippet menu()}<CatalogActions name={title} disabled={busy} allowArchive={false} allowDelete={false} maxNameLength={80} extraActions={[{id:'new',label:'New thread',icon:'square-pen'},{id:'open',label:'Open folder',icon:'folder-open'}]} onaction={(action, name) => action === 'rename' ? onrename(id, name) : action === 'new' ? onnewthread(id) : onopen(id)}/>{/snippet}
            <LucideIcon name="folder" size={32}/><strong>{title}</strong><span>{total} {total === 1 ? 'thread' : 'threads'}</span>
          </CatalogCard>
        {/each}
        {#each remoteMatches as [id, title, owner] (id)}
          {@const total = count(id)}
          <CatalogCard aria-label={`${title} ${total} ${total === 1 ? 'thread' : 'threads'}, files on ${owner?.name ?? 'another machine'}${owner?.online === false ? ', offline' : ''}`} disabled={busy} onclick={() => selected = id}>
            <LucideIcon name="folder" size={32}/><strong>{title}</strong><span>{total} {total === 1 ? 'thread' : 'threads'}</span>
            <small><OwnerChip machine={owner} prefix="files on" /></small>
          </CatalogCard>
        {/each}
      </CardGrid>
      {#if !matches.length && !remoteMatches.length}<p>{search ? 'No matching projects.' : 'Create a project to keep related threads together.'}</p>{/if}
    {/if}
  </PanelPage>
</section>
<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  .back { display: inline-flex; align-items: center; justify-content: center; flex: none; width: 32px; height: 32px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .back:hover { background: var(--faint); color: var(--ink); }
  form { display: flex; align-items: center; gap: 8px; }
  input { flex: 1; min-width: 0; height: 32px; box-sizing: border-box; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  input:focus { outline: none; border-color: var(--muted); }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
</style>

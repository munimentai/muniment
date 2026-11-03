<script>
  // The sidebar under the window's New and Search: the Agents and Artifacts
  // rows, then Projects as core arranges them, pinned rows first and then
  // newest activity first, with archived threads folded at the end, or the
  // search's pages and threads. A thread in no project is its own row, an
  // agent's thread among them. A project row groups its threads and agents
  // and opens to show them. Each
  // thread shows once, and each row states its status in words. A thread row
  // dragged onto a project moves the thread there. The window renders each
  // thread row with its `row` snippet, so a row's menu, rename and selection
  // stay one control everywhere.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProjectItem from './ProjectItem.svelte'
  import { freshIndex } from './sidebar-model.js'
  import { tick } from 'svelte'
  let {
    sidebar, row, fresh = null, showFresh = false, query = '', pages = [], owners = {},
    // A page result opens a page over the thread, so a thread that opens
    // leaves it pressable and only `pagesDisabled` locks it.
    disabled = false, pagesDisabled = disabled, expanded = new Set(), archivedOpen = $bindable(false),
    projectForm = null, projectName = $bindable(''), projectBusy = false, projectError = '',
    // A project just created scrolls into view, takes focus and carries the
    // selection until focus leaves it, wherever core places it in the list.
    reveal = null, onrevealed,
    onpage, ontoggleproject, onnewthread, onsettings, onrenameproject, onopenfolder, ondeleteproject, ongroup, onnewproject, onsaveproject, oncancelproject, onretryprojects, library, footer,
    // A thread row dropped on a project of this computer moves there.
    dragType = '', onmovethread,
  } = $props()
  const searching = $derived(!!query.trim())
  const matchedPages = $derived(searching ? pages.filter((page) => page.label.toLowerCase().includes(query.trim().toLowerCase())) : [])
  const freshAt = $derived(showFresh ? freshIndex(sidebar) : -1)
  const kbd = (shortcut) => shortcut.replace('Meta+', '⌘ ').replace('Control+', 'Ctrl ')
  // The name field takes focus as it opens, after a menu or the palette that
  // opened it returns focus, with its text selected so typing replaces the name.
  const focusField = (node) => { void tick().then(() => { node.focus(); node.select(); node.closest('form')?.scrollIntoView?.({ block: 'nearest' }) }) }
  let projectList = $state()
  let marked = $state(null)
  $effect(() => {
    const id = reveal
    if (!id || searching || !sidebar.projects.some((project) => project.id === id)) return
    void tick().then(() => {
      const title = [...(projectList?.querySelectorAll('[data-project-id]') ?? [])].find((entry) => entry.dataset.projectId === id)?.querySelector('[data-project-title]')
      if (!title) return
      title.scrollIntoView?.({ block: 'nearest' })
      title.focus()
      marked = id
      title.addEventListener('blur', () => { if (marked === id) marked = null }, { once: true })
      onrevealed?.(id)
    })
  })
</script>

<div>
  {#if searching}
    <section class="side-section" aria-label="Search results">
      {#if matchedPages.length}
        <h3>Pages</h3>
        <div class="page-results">{#each matchedPages as page (page.id)}<button class="page-result" aria-label={page.label} disabled={pagesDisabled} onclick={() => onpage(page.id)}><LucideIcon name={page.icon} /><span>{page.label}</span>{#if page.shortcut}<kbd>{kbd(page.shortcut)}</kbd>{/if}</button>{/each}</div>
      {/if}
      <h3>Threads</h3>
      <ul class="thread-list" aria-label="Matching threads">{#each sidebar.results as item (item.thread_id)}{@render row(item)}{/each}</ul>
    </section>
  {:else}
    {#if library}<div class="library">{@render library()}</div>{/if}
    <section class="side-section" aria-label="Projects">
      <div class="section-head">
        <h3>Projects</h3>
        <!-- The tooltip sits below the button, over the name field, so it hides while the form is open. -->
        <button type="button" class="section-control" class:tipless={!!projectForm} aria-label="New project" data-tooltip="New project" disabled={disabled || projectBusy} onclick={onnewproject}><LucideIcon name="plus" size={14} /></button>
      </div>
      <!-- New project opens its field above the list. Rename opens it in place of the project's row, where the project sits. -->
      {#snippet projectField()}{#key projectForm}
        <form class="row-rename" onsubmit={(event) => { event.preventDefault(); onsaveproject() }}>
          <input aria-label="Project name" placeholder="Project name" maxlength="80" spellcheck="false" autocorrect="off" autocapitalize="off" autocomplete="off" use:focusField bind:value={projectName} disabled={projectBusy} />
          <button type="submit" disabled={projectBusy || !projectName.trim()}>{projectForm === 'new' ? 'Create' : 'Save'}</button>
          <button type="button" disabled={projectBusy} onclick={oncancelproject}>Cancel</button>
        </form>
      {/key}{/snippet}
      {#if projectForm && !sidebar.projects.some((project) => project.kind !== 'thread' && project.id === projectForm)}{@render projectField()}{/if}
      <ul class="thread-list" aria-label="Projects list" bind:this={projectList}>
        {#each sidebar.projects as project, index (project.id)}
          {#if index === freshAt}{@render fresh?.()}{/if}
          {#if project.kind === 'thread'}
            {@render row(project.threads[0])}
          {:else}
            <li class="project-entry" data-project-id={project.id}>
              {#if projectForm === project.id}{@render projectField()}{:else}
              <ProjectItem current={marked === project.id} name={project.name} triggers={project.triggers} status={project.status} statusLabel={project.status_label} owner={owners[project.id] ?? null} expanded={expanded.has(project.id)} disabled={disabled || projectBusy}
                ontoggle={() => ontoggleproject(project.id)} onnew={() => onnewthread(project.id)} onsettings={() => onsettings(project.id)}
                onrename={project.kind === 'project' && !project.owner ? () => onrenameproject(project.id, project.name) : undefined}
                onopen={project.kind === 'project' && !project.owner ? () => onopenfolder(project.id) : undefined}
                ondelete={ondeleteproject && project.kind === 'project' && !project.owner ? () => ondeleteproject(project) : undefined}
                ongroup={ongroup ? () => ongroup(project.id) : undefined}
                {dragType} ondropthread={onmovethread && project.kind === 'project' && !project.owner ? (threadId) => onmovethread(threadId, project.id) : undefined} />
              {/if}
              {#if expanded.has(project.id) && project.threads.length}<ul class="thread-list project-threads" aria-label={`${project.name} threads`}>{#each project.threads as item (item.thread_id)}{@render row(item)}{/each}</ul>{/if}
            </li>
          {/if}
        {/each}
        {#if freshAt >= sidebar.projects.length}{@render fresh?.()}{/if}
      </ul>
      {#if projectError}<p class="side-empty" role="status">{projectError} <button type="button" onclick={onretryprojects}>Retry</button></p>{/if}
      {#if sidebar.archived.length}
        <button type="button" class="section-toggle" aria-expanded={archivedOpen} onclick={() => archivedOpen = !archivedOpen}><span>{archivedOpen ? 'Hide archived' : `Show archived (${sidebar.archived.length})`}</span></button>
        {#if archivedOpen}<ul class="thread-list" aria-label="Archived threads">{#each sidebar.archived as item (item.thread_id)}{@render row(item)}{/each}</ul>{/if}
      {/if}
    </section>
  {/if}
  {@render footer?.()}
</div>

<style>
  .thread-list { margin: 0; padding: 0; list-style: none; }
  .side-empty { margin: 0; padding: 4px 8px; color: var(--muted); font-size: var(--text-12); }
  .side-empty button { padding: 0 4px; border: 0; background: transparent; color: var(--ink); font: inherit; text-decoration: underline; cursor: pointer; }
  .row-rename { display: flex; flex-wrap: wrap; gap: 4px; padding: 4px; }
  .row-rename input { width: 100%; min-width: 0; font: inherit; }
  .row-rename button { padding: 2px 6px; font-size: var(--text-12); }
  .page-result { display: flex; align-items: center; gap: 8px; width: 100%; min-height: var(--row-height); padding: var(--row-pad) 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .page-result:hover { background: var(--faint); }
  .page-result span { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; }
  .section-control::after { content: attr(data-tooltip); position: absolute; right: 0; top: calc(100% + 4px); z-index: 4; padding: 4px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; opacity: 0; visibility: hidden; }
  .section-control:hover::after, .section-control:focus-visible::after { opacity: 1; visibility: visible; transition: opacity 80ms ease 350ms; }
  .section-control.tipless::after { display: none; }
  :global(:root[data-motion="reduce"]) .section-control:hover::after { transition: none; }
  .side-section { display: grid; grid-template-columns: minmax(0, 1fr); gap: 1px; margin-top: 10px; }
  .side-section h3, .section-head h3 { margin: 0; padding: 2px 8px; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .section-head { display: flex; align-items: center; gap: 2px; padding-right: 2px; }
  .section-head h3 { flex: 1; }
  .section-control { position: relative; display: inline-grid; place-items: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .section-control:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .section-control:disabled { opacity: .5; cursor: default; }
  .section-toggle { display: flex; align-items: center; gap: 4px; width: 100%; min-height: 24px; padding: 2px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-12) var(--font-human); text-align: left; cursor: pointer; }
  .section-toggle span { flex: 1; }
  .section-toggle:hover { background: var(--faint); color: var(--ink); }
  .project-threads { margin-left: 16px; }
  .project-entry { display: grid; grid-template-columns: minmax(0, 1fr); gap: 1px; }
  .library { display: grid; gap: 1px; margin-top: 10px; }
  .page-results { display: grid; gap: 1px; }
  .page-results kbd { flex: none; color: var(--muted); font: var(--text-12) var(--font-mono); }
</style>

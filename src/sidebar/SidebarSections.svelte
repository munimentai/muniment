<script>
  // The sidebar's threads as core arranges them, under the window's New and
  // Search: Needs you, Working, Pinned, Projects and Recent, with settled and
  // archived threads folded at the foot, or the search's pages and threads.
  // Every thread sits in one project, and each row states its status in
  // words. The window renders each row with its `row` snippet, so a row's menu, rename and selection stay one
  // control everywhere. The sections hold their order: a row moves only when
  // its status changes, and a folded section stays folded until the user
  // opens it.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProjectItem from './ProjectItem.svelte'
  let {
    sidebar, row, fresh = null, query = '', pages = [], owners = {},
    disabled = false, expanded = new Set(), filter = 'all', workingOpen = $bindable(true), settledOpen = $bindable(false), archivedOpen = $bindable(false),
    projectForm = null, projectName = $bindable(''), projectBusy = false, projectError = '',
    onpage, ontoggleproject, onnewthread, onsettings, onrenameproject, onopenfolder, ongroup, onnewproject, onsaveproject, oncancelproject, onretryprojects, onfilter, library, footer,
  } = $props()
  const searching = $derived(!!query.trim())
  const matchedPages = $derived(searching ? pages.filter((page) => page.label.toLowerCase().includes(query.trim().toLowerCase())) : [])
  const kbd = (shortcut) => shortcut.replace('Meta+', '⌘ ').replace('Control+', 'Ctrl ')
</script>

<div>
  {#if searching}
    <section class="side-section" aria-label="Search results">
      {#if matchedPages.length}
        <h3>Pages</h3>
        <div class="page-results">{#each matchedPages as page (page.id)}<button class="page-result" aria-label={page.label} {disabled} onclick={() => onpage(page.id)}><LucideIcon name={page.icon} /><span>{page.label}</span>{#if page.shortcut}<kbd>{kbd(page.shortcut)}</kbd>{/if}</button>{/each}</div>
      {/if}
      <h3>Threads</h3>
      <ul class="thread-list" aria-label="Matching threads">{#each sidebar.results as item (item.thread_id)}{@render row(item)}{/each}</ul>
    </section>
  {:else}
    {#if sidebar.needs_you.length}
      <section class="side-section" aria-label="Needs you">
        <h3>Needs you</h3>
        <ul class="thread-list" aria-label="Threads that need you">{#each sidebar.needs_you as item (item.thread_id)}{@render row(item)}{/each}</ul>
      </section>
    {/if}
    {#if sidebar.working.length}
      <section class="side-section" aria-label="Working">
        <button type="button" class="section-toggle" aria-expanded={workingOpen} onclick={() => workingOpen = !workingOpen}><span>Working ({sidebar.working.length})</span><LucideIcon name={workingOpen ? 'chevron-down' : 'chevron-right'} size={14} /></button>
        {#if workingOpen}<ul class="thread-list" aria-label="Working threads">{#each sidebar.working as item (item.thread_id)}{@render row(item)}{/each}</ul>{/if}
      </section>
    {/if}
    {#if sidebar.pinned.length}
      <section class="side-section" aria-label="Pinned">
        <h3>Pinned</h3>
        <ul class="thread-list" aria-label="Pinned threads">{#each sidebar.pinned as item (item.thread_id)}{@render row(item)}{/each}</ul>
      </section>
    {/if}
    <section class="side-section" aria-label="Projects">
      <div class="section-head">
        <h3>Projects</h3>
        <button type="button" class="section-control" aria-label="Projects with triggers" aria-pressed={filter === 'triggers'} data-tooltip="With triggers" onclick={() => onfilter(filter === 'triggers' ? 'all' : 'triggers')}><LucideIcon name="bot" size={14} /></button>
        <button type="button" class="section-control" aria-label="New project" data-tooltip="New project" disabled={disabled || projectBusy} onclick={onnewproject}><LucideIcon name="plus" size={14} /></button>
      </div>
      {#if projectForm}
        <form class="row-rename" onsubmit={(event) => { event.preventDefault(); onsaveproject() }}>
          <input aria-label="Project name" placeholder="Project name" maxlength="80" bind:value={projectName} disabled={projectBusy} />
          <button type="submit" disabled={projectBusy || !projectName.trim()}>{projectForm === 'new' ? 'Create' : 'Save'}</button>
          <button type="button" disabled={projectBusy} onclick={oncancelproject}>Cancel</button>
        </form>
      {/if}
      {#each sidebar.projects as project (project.id)}
        <ProjectItem name={project.name} triggers={project.triggers} kind={project.kind} owner={owners[project.id] ?? null} expanded={expanded.has(project.id)} disabled={disabled || projectBusy}
          ontoggle={() => ontoggleproject(project.id)} onnew={() => onnewthread(project.id)} onsettings={() => onsettings(project.id)}
          onrename={project.kind === 'project' && !project.owner ? () => onrenameproject(project.id, project.name) : undefined}
          onopen={project.kind === 'project' && !project.owner ? () => onopenfolder(project.id) : undefined}
          ongroup={ongroup ? () => ongroup(project.id) : undefined} />
        {#if expanded.has(project.id) && project.threads.length}<ul class="thread-list project-threads" aria-label={`${project.name} threads`}>{#each project.threads as item (item.thread_id)}{@render row(item)}{/each}</ul>{/if}
      {/each}
      {#if filter === 'triggers' && !sidebar.projects.length}<p class="side-empty">No project has a trigger yet.</p>{/if}
      {@render library?.()}
      {#if projectError}<p class="side-empty" role="status">{projectError} <button type="button" onclick={onretryprojects}>Retry</button></p>{/if}
    </section>
    <section class="side-section" aria-label="Recent">
      <h3>Recent</h3>
      <ul class="thread-list" aria-label="Recent threads">{@render fresh?.()}{#each sidebar.recent as item (item.thread_id)}{@render row(item)}{/each}</ul>
      {#if sidebar.settled.length}
        <button type="button" class="section-toggle fold" aria-expanded={settledOpen} onclick={() => settledOpen = !settledOpen}><span>{settledOpen ? 'Hide settled' : `Show settled (${sidebar.settled.length})`}</span></button>
        {#if settledOpen}<ul class="thread-list" aria-label="Settled threads">{#each sidebar.settled as item (item.thread_id)}{@render row(item)}{/each}</ul>{/if}
      {/if}
      {#if sidebar.archived.length}
        <button type="button" class="section-toggle fold" aria-expanded={archivedOpen} onclick={() => archivedOpen = !archivedOpen}><span>{archivedOpen ? 'Hide archived' : `Show archived (${sidebar.archived.length})`}</span></button>
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
  :global(:root[data-motion="reduce"]) .section-control:hover::after { transition: none; }
  .side-section { display: grid; grid-template-columns: minmax(0, 1fr); gap: 1px; margin-top: 10px; }
  .side-section h3, .section-head h3 { margin: 0; padding: 2px 8px; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .section-head { display: flex; align-items: center; gap: 2px; padding-right: 2px; }
  .section-head h3 { flex: 1; }
  .section-control { position: relative; display: inline-grid; place-items: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .section-control:hover:not(:disabled), .section-control[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  .section-control:disabled { opacity: .5; cursor: default; }
  .section-toggle { display: flex; align-items: center; gap: 4px; width: 100%; min-height: 24px; padding: 2px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); text-align: left; cursor: pointer; }
  .section-toggle span { flex: 1; }
  .section-toggle.fold { font-weight: normal; }
  .section-toggle:hover { background: var(--faint); color: var(--ink); }
  .project-threads { margin-left: 16px; }
  .page-results { display: grid; gap: 1px; }
  .page-results kbd { flex: none; color: var(--muted); font: var(--text-12) var(--font-mono); }
</style>

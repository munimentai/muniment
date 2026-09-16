<script>
  // The record panel: the rail column's second occupant. Its header is one
  // mono row, the company picker, the crumb and Maximize. The body walks
  // three levels, the kind list, one kind's table, one record, and every
  // level generates from the kind row the runtime answers.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import RecordBoard from './RecordBoard.svelte'
  import RecordForm from './RecordForm.svelte'
  import RecordImport from './RecordImport.svelte'
  import RecordTable from './RecordTable.svelte'
  import RecordView from './RecordView.svelte'
  import { currentCompany, kindLabel, kindSummary, orderKinds, recordErrorLine, validCompanyName } from './record-panel-state.js'
  import { askSql, diffLines, viewData, viewSettings, viewsFor } from './record-table-state.js'

  let { tauri, maximized = false, ontogglemaximized, onask } = $props()

  let companies = $state([])
  let company = $derived(currentCompany(companies))
  let kinds = $state([])
  let selectedKind = $state(null)
  let kind = $derived(kinds.find((candidate) => candidate.name === selectedKind) ?? null)
  let page = $state(null)
  let sort = $state({ sort: 'updated_at', descending: true })
  let search = $state('')
  let layout = $state('table')
  let stateFilter = $state(null)
  let views = $state([])
  let selectedView = $state('')
  let savingView = $state(false)
  let viewName = $state('')
  let pendingView = $state(null)
  const hasStates = $derived(Array.isArray(kind?.states) && kind.states.length > 0)
  let detail = $state(null)
  let creating = $state(false)
  // null, or { mapping } for a run of a committed mapping record.
  let importing = $state(null)
  let error = $state(null)
  let loading = $state(false)
  let newCompanyName = $state('')
  let loadVersion = 0

  $effect(() => {
    void loadCompanies()
  })

  // A stale answer never lands: each load carries its version.
  async function loadCompanies(preferredCompanyId) {
    if (!tauri) return
    const version = ++loadVersion
    loading = true
    error = null
    try {
      const listed = await tauri.invoke('record_companies')
      if (version !== loadVersion) return
      companies = Array.isArray(listed?.companies) ? listed.companies : []
      const chosen = (preferredCompanyId && companies.find((candidate) => candidate.id === preferredCompanyId)) || currentCompany(companies)
      if (!chosen) {
        kinds = []
        selectedKind = null
        return
      }
      const answer = await tauri.invoke('record_kinds', { companyId: chosen.id })
      if (version !== loadVersion) return
      if (answer?.error) {
        error = recordErrorLine(answer.error.message)
        kinds = []
        return
      }
      kinds = orderKinds(answer?.kinds)
      if (!kinds.some((candidate) => candidate.name === selectedKind)) selectedKind = null
    } catch (failure) {
      if (version !== loadVersion) return
      error = recordErrorLine(failure)
      kinds = []
    } finally {
      if (version === loadVersion) loading = false
    }
  }

  async function selectCompany(companyId) {
    if (!tauri || !companyId || companyId === company?.id) return
    error = null
    try {
      await tauri.invoke('record_company_select', { companyId })
    } catch (failure) {
      error = recordErrorLine(failure)
      return
    }
    selectedKind = null
    detail = null
    page = null
    await loadCompanies(companyId)
  }

  async function createCompany(event) {
    event?.preventDefault?.()
    if (!tauri || !validCompanyName(newCompanyName)) return
    const name = newCompanyName.trim()
    error = null
    try {
      const created = await tauri.invoke('record_company_create', { name })
      newCompanyName = ''
      const id = created?.company?.id
      if (id && companies.length > 0) await tauri.invoke('record_company_select', { companyId: id })
      await loadCompanies(id)
    } catch (failure) {
      error = recordErrorLine(failure)
    }
  }

  async function openKind(name) {
    selectedKind = name
    detail = null
    creating = false
    importing = null
    sort = { sort: 'updated_at', descending: true }
    search = ''
    stateFilter = null
    layout = 'table'
    selectedView = ''
    savingView = false
    pendingView = null
    await Promise.all([loadPage(), loadViews()])
  }

  // Saved views are `view` records over this kind.
  async function loadViews() {
    if (!tauri || !company || !selectedKind) return
    try {
      const answer = await tauri.invoke('record_query', { companyId: company.id, kind: 'view', limit: 200, sort: 'title', descending: false })
      views = answer?.error ? [] : viewsFor(answer?.page?.rows, selectedKind)
    } catch {
      views = []
    }
  }

  async function applyView(viewId) {
    selectedView = viewId
    const row = views.find((candidate) => candidate.id === viewId)
    if (!row) return
    const settings = viewSettings(row)
    layout = settings.layout
    sort = settings.sort
    stateFilter = settings.state
    search = settings.search
    await loadPage()
  }

  async function proposeView(event) {
    event?.preventDefault?.()
    const name = viewName.trim()
    if (!name || !kind) return
    const answer = await propose({ op: 'create', kind: 'view', data: viewData(name, kind, { layout, sort, state: stateFilter, search }) })
    if (answer?.error) {
      pendingView = { error: answer.error.message ?? 'The record refused the view.' }
      return
    }
    pendingView = { proposal: answer.proposal?.id, lines: diffLines(answer.proposal?.diff) }
  }

  async function commitView() {
    if (!pendingView?.proposal) return
    const answer = await commit(pendingView.proposal)
    if (answer?.error) {
      pendingView = { error: answer.error.message ?? 'The commit failed.' }
      return
    }
    const id = answer?.result?.entity_ids?.[0]
    pendingView = null
    savingView = false
    viewName = ''
    await loadViews()
    if (id) selectedView = id
  }

  function ask() {
    if (!kind) return
    onask?.(askSql(kind, { sort, state: stateFilter, search }))
  }

  async function changeStateFilter(value) {
    stateFilter = value || null
    await loadPage()
  }

  async function loadPage() {
    if (!tauri || !company || !selectedKind) return
    const version = ++loadVersion
    loading = true
    error = null
    try {
      const answer = await tauri.invoke('record_query', {
        companyId: company.id,
        kind: selectedKind,
        sort: sort.sort,
        descending: sort.descending,
        state: stateFilter || null,
        search: search.trim() || null,
        limit: 200,
      })
      if (version !== loadVersion) return
      if (answer?.error) {
        error = recordErrorLine(answer.error.message)
        page = null
        return
      }
      page = answer?.page ?? null
    } catch (failure) {
      if (version !== loadVersion) return
      error = recordErrorLine(failure)
    } finally {
      if (version === loadVersion) loading = false
    }
  }

  async function changeSort(next) {
    sort = next
    await loadPage()
  }

  async function changeSearch(text) {
    search = text
    await loadPage()
  }

  async function openEntity(id) {
    if (!tauri || !company) return
    const version = ++loadVersion
    loading = true
    error = null
    try {
      const answer = await tauri.invoke('record_entity', { companyId: company.id, entity: id })
      if (version !== loadVersion) return
      if (answer?.error) {
        error = recordErrorLine(answer.error.message)
        return
      }
      detail = answer?.entity ?? null
      creating = false
      if (detail && detail.kind?.name !== selectedKind) selectedKind = detail.kind.name
    } catch (failure) {
      if (version !== loadVersion) return
      error = recordErrorLine(failure)
    } finally {
      if (version === loadVersion) loading = false
    }
  }

  // propose and commit reach the runtime as the owner. A failure comes back
  // as the body the caller shows, never as a thrown error.
  async function propose(operation) {
    try {
      return await tauri.invoke('record_propose', { companyId: company?.id, operation })
    } catch (failure) {
      return { error: { message: recordErrorLine(failure) } }
    }
  }

  async function commit(proposal) {
    try {
      return await tauri.invoke('record_commit', { companyId: company?.id, proposal })
    } catch (failure) {
      return { error: { message: recordErrorLine(failure) } }
    }
  }

  async function afterCommit(entityId) {
    if (detail && entityId === detail.entity?.id) await openEntity(entityId)
    else await loadPage()
  }

  // The import lands rows on the open kind, so the table reloads when it ends.
  async function afterImport() {
    importing = null
    if (detail) await openEntity(detail.entity.id)
    else await loadPage()
  }

  // A mapping record runs again from its own view. The rows land on the
  // mapping's kind, and the run summary shows in its place.
  function runMapping() {
    if (detail?.kind?.name !== 'mapping') return
    importing = { mapping: detail.entity.id }
  }

  function back() {
    error = null
    if (importing) importing = null
    else if (creating) creating = false
    else if (detail) {
      detail = null
      void loadPage()
    } else if (selectedKind) {
      selectedKind = null
      page = null
    }
  }

  const crumb = $derived.by(() => {
    const parts = []
    if (kind) parts.push(kindLabel(kind.name))
    if (importing && detail) parts.push(detail.entity?.title ?? '', 'run')
    else if (importing) parts.push('import')
    else if (creating) parts.push('new')
    else if (detail) parts.push(detail.entity?.title ?? '')
    return parts
  })
</script>

<aside id="record-panel" class="record-panel" aria-labelledby="record-panel-title" data-testid="record-panel">
  <header class="record-header">
    {#if selectedKind}
      <button type="button" class="record-back" aria-label="Back" onclick={back}><LucideIcon name="chevron-left" size={14} /></button>
    {/if}
    <h2 id="record-panel-title">Record</h2>
    {#if companies.length > 0}
      <label>
        <span class="visually-hidden">Company</span>
        <select class="record-company-picker" aria-label="Company" value={company?.id ?? ''} onchange={(event) => selectCompany(event.currentTarget.value)}>
          {#each companies as candidate (candidate.id)}
            <option value={candidate.id}>{candidate.name}</option>
          {/each}
        </select>
      </label>
    {/if}
    {#each crumb as part, index (index)}
      <span class="record-crumb" aria-hidden="true">/</span>
      <span class="record-crumb-part">{part}</span>
    {/each}
    <span class="record-header-spacer"></span>
    {#if kind && !creating && !detail && !importing}
      <button type="button" class="record-new" onclick={() => { creating = true }}><LucideIcon name="plus" size={14} /><span>New {kindLabel(kind.name)}</span></button>
    {/if}
    {#if kind && detail && detail.kind?.name === 'mapping' && !importing}
      <button type="button" class="record-new" onclick={runMapping}><LucideIcon name="play" size={14} /><span>Run</span></button>
    {/if}
    <button type="button" class="record-maximize" aria-pressed={maximized} aria-label={maximized ? 'Restore the thread beside the record' : 'Maximize the record over the thread'} onclick={ontogglemaximized}>
      <LucideIcon name={maximized ? 'minimize-2' : 'maximize-2'} size={14} />
      <span>{maximized ? 'Restore' : 'Maximize'}</span>
    </button>
  </header>
  {#if error}
    <p class="record-state" role="alert">{error}</p>
  {:else if loading && companies.length === 0}
    <p class="record-state">Reading the record</p>
  {:else if companies.length === 0}
    <form class="record-create" onsubmit={createCompany}>
      <p class="record-state">No company yet</p>
      <input class="record-company-name" type="text" aria-label="Company name" placeholder="Company name" maxlength="120" bind:value={newCompanyName}>
      <button type="submit" class="record-create-button" disabled={!validCompanyName(newCompanyName)}>Create company</button>
    </form>
  {:else if kind && importing}
    <RecordImport {tauri} companyId={company?.id} kind={importing.mapping && detail?.entity?.data?.kind ? (kinds.find((candidate) => candidate.name === detail.entity.data.kind) ?? kind) : kind} mapping={importing.mapping} {propose} {commit} oncancel={() => { importing = null }} ondone={afterImport} />
  {:else if kind && creating}
    <RecordForm {kind} {propose} {commit} oncancel={() => { creating = false }} oncreated={(id) => { creating = false; void loadPage().then(() => openEntity(id)) }} />
  {:else if kind && detail}
    <RecordView {detail} onopen={openEntity} />
  {:else if kind}
    <div class="record-kind-body">
      <div class="record-toolbar" role="toolbar" aria-label="View">
        {#if hasStates}
          <div class="record-layouts">
            <button type="button" class="record-layout" aria-pressed={layout === 'table'} onclick={() => { layout = 'table' }}>Table</button>
            <button type="button" class="record-layout" aria-pressed={layout === 'board'} onclick={() => { layout = 'board' }}>Board</button>
          </div>
          <label class="record-filter">
            <span class="visually-hidden">State</span>
            <select class="record-select" aria-label="State" value={stateFilter ?? ''} onchange={(event) => changeStateFilter(event.currentTarget.value)}>
              <option value="">every state</option>
              {#each kind.states as state (state)}<option value={state}>{state}</option>{/each}
            </select>
          </label>
        {/if}
        {#if views.length}
          <select class="record-select" aria-label="Saved view" value={selectedView} onchange={(event) => applyView(event.currentTarget.value)}>
            <option value="">views</option>
            {#each views as view (view.id)}<option value={view.id}>{view.title}</option>{/each}
          </select>
        {/if}
        <span class="record-toolbar-spacer"></span>
        <button type="button" class="record-tool" onclick={() => { importing = {} }}>Import</button>
        <button type="button" class="record-tool" aria-pressed={savingView} onclick={() => { savingView = !savingView; pendingView = null }}>Save view</button>
        <button type="button" class="record-tool" onclick={ask}>Ask</button>
      </div>
      {#if savingView}
        <form class="record-save-view" aria-label="Save view" onsubmit={proposeView}>
          {#if pendingView?.error}
            <p class="record-diff-error" role="alert">{pendingView.error}</p>
          {:else if pendingView}
            <div class="record-save-diff" role="group" aria-label="Proposed view">
              {#each pendingView.lines as line (line)}<p class="record-diff-line">{line}</p>{/each}
              <div class="record-save-actions">
                <button type="button" class="record-commit" onclick={commitView}>Commit</button>
                <button type="button" class="record-discard" onclick={() => { pendingView = null }}>Discard</button>
              </div>
            </div>
          {:else}
            <input class="record-view-name" type="text" aria-label="View name" placeholder="View name" maxlength="120" bind:value={viewName}>
            <button type="submit" class="record-commit" disabled={!viewName.trim()}>Propose</button>
          {/if}
        </form>
      {/if}
      {#if layout === 'board' && hasStates}
        <RecordBoard {kind} {page} {loading} onopen={openEntity} {propose} {commit} oncommitted={afterCommit} />
      {:else}
        <RecordTable {kind} {page} {sort} {search} {loading} onsort={changeSort} onsearch={changeSearch} onopen={openEntity} {propose} {commit} oncommitted={afterCommit} />
      {/if}
    </div>
  {:else}
    <nav class="record-kinds" aria-label="Kinds">
      <ul>
        {#each kinds as entry (entry.name)}
          <li>
            <button type="button" class="record-kind" class:own={entry.name.startsWith('x_')} onclick={() => openKind(entry.name)}>
              <span class="record-kind-name">{kindLabel(entry.name)}</span>
              <span class="record-kind-summary">{kindSummary(entry)}</span>
            </button>
          </li>
        {/each}
      </ul>
    </nav>
  {/if}
</aside>

<style>
  .record-panel { grid-area: rail; min-width: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); padding: 14px 16px 16px; overflow: hidden; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .record-header { display: flex; align-items: center; gap: 8px; min-height: 24px; padding-bottom: 12px; border-bottom: 1px solid var(--border); font: var(--text-13) var(--font-mono); }
  .record-header h2 { margin: 0; font: 600 var(--text-15)/1.3 var(--font-body); }
  .record-back { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 1px solid transparent; border-radius: var(--radius-control); color: var(--ink); cursor: pointer; }
  .record-back:hover, .record-new:hover, .record-maximize:hover { background: var(--faint); }
  .record-company-picker { max-width: 200px; height: 24px; padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; }
  .record-crumb { color: var(--muted); }
  .record-crumb-part { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 200px; }
  .record-header-spacer { flex: 1; }
  .record-new, .record-maximize { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 6px; border: 1px solid transparent; border-radius: var(--radius-control); color: var(--ink); font: inherit; cursor: pointer; white-space: nowrap; }
  .record-state { margin: 12px 0 0; color: var(--muted); font: var(--text-13) var(--font-mono); }
  .record-create { display: grid; gap: 8px; align-content: start; }
  .record-company-name { height: 28px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-body); }
  .record-company-name:focus { outline: none; border-color: var(--muted); }
  .record-create-button { justify-self: start; height: 28px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--ink); color: var(--paper); font: var(--text-13) var(--font-body); cursor: pointer; }
  .record-create-button:disabled { background: var(--faint); color: var(--muted); cursor: default; }
  .record-kind-body { display: grid; grid-template-rows: auto auto minmax(0, 1fr); min-height: 0; }
  .record-toolbar { display: flex; align-items: center; gap: 8px; padding-top: 10px; font: var(--text-12) var(--font-mono); }
  .record-layouts { display: inline-flex; border: 1px solid var(--border); border-radius: var(--radius-control); overflow: hidden; }
  .record-layout { height: 24px; padding: 0 8px; border: 0; background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); cursor: pointer; }
  .record-layout[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  .record-filter { display: inline-flex; }
  .record-select { height: 24px; max-width: 180px; padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); }
  .record-toolbar-spacer { flex: 1; }
  .record-tool { height: 24px; padding: 0 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); cursor: pointer; }
  .record-tool:hover, .record-tool[aria-pressed="true"] { background: var(--faint); }
  .record-save-view { display: flex; align-items: center; gap: 8px; padding-top: 8px; }
  .record-view-name { flex: 1; height: 26px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-body); }
  .record-view-name:focus { outline: none; border-color: var(--muted); }
  .record-save-diff { display: grid; gap: 4px; }
  .record-save-actions { display: flex; gap: 8px; }
  .record-diff-line { margin: 0; font: var(--text-12) var(--font-mono); }
  .record-diff-error { margin: 0; color: var(--oxide); font: var(--text-12) var(--font-mono); }
  .record-commit, .record-discard { height: 26px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-body); cursor: pointer; }
  .record-commit { background: var(--ink); color: var(--paper); }
  .record-commit:disabled { background: var(--faint); color: var(--muted); cursor: default; }
  .record-discard { background: var(--surface); color: var(--ink); }
  .record-kinds { min-height: 0; overflow-y: auto; }
  .record-kinds ul { margin: 0; padding: 0; list-style: none; }
  .record-kinds li + li { border-top: 1px solid var(--border); }
  .record-kind { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; width: 100%; min-height: 28px; padding: 0 6px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); text-align: left; font: var(--text-13) var(--font-body); cursor: pointer; }
  .record-kind:hover { background: var(--faint); }
  .record-kind.own .record-kind-name::after { content: " (own)"; color: var(--muted); }
  .record-kind-summary { color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; }
</style>

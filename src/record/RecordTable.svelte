<script>
  import { panelScroll } from '../lib/panel-scroll.js'
  // One kind's rows. Columns generate from the kind, typed values read in
  // mono and titles in the body face, a header click sorts, a row click opens
  // the record, and a double-click on a cell edits it: Enter proposes, the
  // diff shows under the row, Enter again commits and Escape drops it.
  import { cellText, cellValue, coerceInput, diffLines, editOperation, nextSort, tableColumns } from './record-table-state.js'

  let { kind, page, sort, search = '', loading = false, onsort, onsearch, onopen, propose, commit, oncommitted } = $props()

  const columns = $derived(tableColumns(kind))
  let editing = $state(null)
  let pending = $state(null)
  let searchDraft = $state(search)
  let editInput = $state()

  $effect(() => {
    searchDraft = search
  })

  $effect(() => {
    if (editing && editInput) editInput.focus()
  })

  function startEdit(row, column) {
    if (column.base || pending) return
    editing = { rowId: row.id, key: column.key, text: cellText(cellValue(row, column), column) }
  }

  async function submitEdit(row, column) {
    if (!editing) return
    const value = coerceInput(editing.text, column)
    const operation = editOperation(row, column, value)
    editing = null
    const answer = await propose(operation)
    if (answer?.error) {
      pending = { rowId: row.id, error: answer.error.message ?? 'The record refused the change.' }
      return
    }
    const proposal = answer?.proposal
    pending = {
      rowId: row.id,
      proposal: proposal?.id,
      lines: diffLines(proposal?.diff),
      warnings: proposal?.warnings ?? [],
    }
  }

  async function commitPending() {
    if (!pending?.proposal) return
    const answer = await commit(pending.proposal)
    if (answer?.error) {
      pending = { rowId: pending.rowId, error: answer.error.message ?? 'The commit failed.' }
      return
    }
    const id = pending.rowId
    pending = null
    await oncommitted?.(id)
  }

  function discard() {
    editing = null
    pending = null
  }

  function editKeydown(event, row, column) {
    if (event.key === 'Enter') {
      event.preventDefault()
      void submitEdit(row, column)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      discard()
    }
  }

  function pendingKeydown(event) {
    if (event.key === 'Escape') {
      event.preventDefault()
      discard()
    }
  }

  function submitSearch(event) {
    event.preventDefault()
    void onsearch?.(searchDraft)
  }
</script>

<div class="record-table">
  <form class="record-search" role="search" onsubmit={submitSearch}>
    <input type="search" class="record-search-input" aria-label="Search {kind?.name ?? ''}" placeholder="Search" bind:value={searchDraft}>
    <span class="record-count">{page ? `${page.total} ${page.total === 1 ? 'record' : 'records'}` : ''}</span>
  </form>
  {#if page && page.rows.length === 0 && !loading}
    <p class="record-empty">No {kind?.name} records yet</p>
  {:else if page}
    <div class="record-scroll" use:panelScroll>
      <table class="record-grid" aria-label="{kind?.name} records">
        <thead>
          <tr>
            {#each columns as column (column.key)}
              <th scope="col" class:mono={column.mono} aria-sort={sort?.sort === column.key ? (sort.descending ? 'descending' : 'ascending') : 'none'}>
                <button type="button" class="record-sort" onclick={() => onsort?.(nextSort(sort, column.key))}>{column.label}{sort?.sort === column.key ? (sort.descending ? ' v' : ' ^') : ''}</button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each page.rows as row (row.id)}
            <tr class="record-row" class:pending={pending?.rowId === row.id}>
              {#each columns as column (column.key)}
                {#if editing?.rowId === row.id && editing.key === column.key}
                  <td class="record-cell editing" class:mono={column.mono}>
                    <input class="record-cell-input" aria-label="{column.label} for {row.title}" bind:this={editInput} bind:value={editing.text} onkeydown={(event) => editKeydown(event, row, column)} onblur={discard}>
                  </td>
                {:else if column.key === 'title'}
                  <td class="record-cell title"><button type="button" class="record-open" onclick={() => onopen?.(row.id)}>{row.title}</button></td>
                {:else}
                  <td class="record-cell" class:mono={column.mono} ondblclick={() => startEdit(row, column)} title={column.base ? undefined : 'Double-click to edit'}>{cellText(cellValue(row, column), column)}</td>
                {/if}
              {/each}
            </tr>
            {#if pending?.rowId === row.id}
              <tr class="record-diff">
                <td colspan={columns.length}>
                  {#if pending.error}
                    <p class="record-diff-error" role="alert">{pending.error}</p>
                    <button type="button" class="record-discard" onclick={discard}>Discard</button>
                  {:else}
                    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                    <div class="record-diff-body" role="group" aria-label="Proposed change" onkeydown={pendingKeydown}>
                      {#each pending.lines as line (line)}<p class="record-diff-line">{line}</p>{/each}
                      {#each pending.warnings as warning (warning)}<p class="record-diff-warning">{warning}</p>{/each}
                      <div class="record-diff-actions">
                        <!-- svelte-ignore a11y_autofocus -->
                        <button type="button" class="record-commit" autofocus onclick={commitPending}>Commit</button>
                        <button type="button" class="record-discard" onclick={discard}>Discard</button>
                      </div>
                    </div>
                  {/if}
                </td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <p class="record-empty">{loading ? 'Reading the record' : ''}</p>
  {/if}
</div>

<style>
  .record-table { display: grid; grid-template-rows: auto minmax(0, 1fr); min-height: 0; }
  .record-search { display: flex; align-items: center; gap: 10px; padding: 10px 0; }
  .record-search-input { flex: 1; height: 26px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  .record-search-input:focus { outline: none; border-color: var(--muted); }
  .record-count { color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  .record-scroll { min-height: 0; overflow: auto; }
  .record-grid { width: 100%; border-collapse: collapse; font: var(--text-13) var(--font-human); }
  .record-grid th { position: sticky; top: 0; z-index: 1; padding: 0; text-align: left; background: var(--surface); border-bottom: 1px solid var(--border); font: var(--text-12) var(--font-mono); color: var(--muted); white-space: nowrap; }
  .record-sort { height: 28px; padding: 0 8px; border: 0; background: transparent; color: inherit; font: inherit; cursor: pointer; text-align: left; width: 100%; }
  .record-sort:hover { color: var(--ink); background: var(--faint); }
  .record-cell { height: 28px; padding: 0 8px; border-bottom: 1px solid var(--border); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 280px; vertical-align: middle; }
  .record-cell.mono { font: var(--text-12) var(--font-mono); }
  .record-cell.title { font-weight: 500; }
  .record-cell.editing { padding: 0 2px; }
  .record-cell-input { width: 100%; height: 24px; padding: 0 6px; border: 1px solid var(--muted); border-radius: var(--radius-chip); background: var(--surface); color: var(--ink); font: inherit; }
  .record-cell-input:focus { outline: none; }
  .record-open { padding: 0; border: 0; background: transparent; color: var(--ink); font: inherit; cursor: pointer; text-align: left; }
  .record-open:hover { text-decoration: underline; }
  .record-row:hover .record-cell { background: var(--faint); }
  .record-row.pending .record-cell { border-bottom-color: transparent; }
  .record-diff td { padding: 6px 8px 10px; border-bottom: 1px solid var(--border); }
  .record-diff-body { display: grid; gap: 4px; }
  .record-diff-line, .record-diff-warning, .record-diff-error { margin: 0; font: var(--text-12) var(--font-mono); }
  .record-diff-line { color: var(--ink); }
  .record-diff-warning { color: var(--ochre); }
  .record-diff-error { color: var(--oxide); }
  .record-diff-actions { display: flex; gap: 8px; margin-top: 4px; }
  .record-commit, .record-discard { height: 24px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-human); cursor: pointer; }
  .record-commit { background: var(--ink); color: var(--paper); }
  .record-discard { background: var(--surface); color: var(--ink); }
  .record-empty { margin: 12px 0 0; color: var(--muted); font: var(--text-13) var(--font-mono); }
</style>

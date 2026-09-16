<script>
  // The board: one column per state of the kind. A card is a title and one
  // mono line. Dragging a card onto another column proposes the state change,
  // the diff shows at the top of the board with Commit and Discard, and a
  // card in flight shows no color.
  import { boardColumns, cellText, diffLines, moveOperation } from './record-table-state.js'

  let { kind, page, loading = false, onopen, propose, commit, oncommitted } = $props()

  const columns = $derived(boardColumns(kind, page?.rows))
  let dragging = $state(null)
  let over = $state(null)
  let pending = $state(null)
  let press = null
  let moved = null

  // The webview keeps native drag sessions for file drops, so a card moves
  // with pointer events: press, move past six pixels, release over a column.
  function columnAt(target) {
    return target?.closest?.('.record-column')?.dataset?.column ?? null
  }

  function pointerDown(event, row) {
    if ((event.button ?? 0) > 0 || !kind?.schema?.stateProperty || pending) return
    press = { rowId: row.id, x: event.clientX ?? 0, y: event.clientY ?? 0 }
  }

  function pointerMove(event) {
    if (!press) return
    if (!dragging) {
      if (Math.hypot((event.clientX ?? 0) - press.x, (event.clientY ?? 0) - press.y) < 6) return
      dragging = press.rowId
    }
    event.preventDefault?.()
    over = columnAt(event.target)
  }

  async function pointerUp(event) {
    if (!press) return
    const rowId = press.rowId
    const label = dragging ? columnAt(event.target) : null
    const wasDragging = !!dragging
    press = null
    dragging = null
    over = null
    if (!wasDragging) return
    moved = rowId
    const column = columns.find((candidate) => candidate.label === label)
    if (column) await move(rowId, column)
  }

  function cancelDrag() {
    press = null
    dragging = null
    over = null
  }

  function open(row) {
    if (moved === row.id) {
      moved = null
      return
    }
    onopen?.(row.id)
  }

  async function move(id, column) {
    const row = page?.rows?.find((candidate) => candidate.id === id)
    if (!row || column.state === null) return
    const operation = moveOperation(row, kind, column.state)
    if (!operation) return
    const answer = await propose(operation)
    if (answer?.error) {
      pending = { rowId: row.id, error: answer.error.message ?? 'The record refused the move.' }
      return
    }
    pending = { rowId: row.id, proposal: answer.proposal?.id, lines: diffLines(answer.proposal?.diff), warnings: answer.proposal?.warnings ?? [] }
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
    pending = null
  }

  function keydown(event) {
    if (event.key === 'Escape' && pending) {
      event.preventDefault()
      discard()
    }
  }
</script>

<svelte:window onpointermove={pointerMove} onpointerup={pointerUp} onpointercancel={cancelDrag} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="record-board" class:dragging={!!dragging} role="region" aria-label="{kind?.name} board" onkeydown={keydown}>
  {#if pending}
    <div class="record-board-pending" role="group" aria-label="Proposed move">
      {#if pending.error}
        <p class="record-diff-error" role="alert">{pending.error}</p>
        <button type="button" class="record-discard" onclick={discard}>Discard</button>
      {:else}
        {#each pending.lines as line (line)}<p class="record-diff-line">{line}</p>{/each}
        {#each pending.warnings as warning (warning)}<p class="record-diff-warning">{warning}</p>{/each}
        <div class="record-board-actions">
          <!-- svelte-ignore a11y_autofocus -->
          <button type="button" class="record-commit" autofocus onclick={commitPending}>Commit</button>
          <button type="button" class="record-discard" onclick={discard}>Discard</button>
        </div>
      {/if}
    </div>
  {/if}
  {#if columns.length === 0}
    <p class="record-empty">{loading ? 'Reading the record' : `No ${kind?.name} records yet`}</p>
  {:else}
    <div class="record-columns">
      {#each columns as column (column.label)}
        <section class="record-column" class:over={over === column.label} class:unset={column.state === null} aria-label={column.label} data-column={column.label}>
          <h4 class="record-column-title">{column.label} <span class="record-column-count">{column.rows.length}</span></h4>
          <ul class="record-cards">
            {#each column.rows as row (row.id)}
              <li>
                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                <div class="record-card" class:dragging={dragging === row.id} role="listitem" onpointerdown={(event) => pointerDown(event, row)}>
                  <button type="button" class="record-card-title" onclick={() => open(row)}>{row.title}</button>
                  <p class="record-card-meta">{cellText(row.updated_at, { type: 'date-time' })}</p>
                </div>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
</div>

<style>
  .record-board { display: grid; grid-template-rows: auto minmax(0, 1fr); min-height: 0; padding-top: 10px; }
  .record-board-pending { display: grid; gap: 4px; margin-bottom: 10px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); }
  .record-diff-line, .record-diff-warning, .record-diff-error { margin: 0; font: var(--text-12) var(--font-mono); }
  .record-diff-warning { color: var(--ochre); }
  .record-diff-error { color: var(--oxide); }
  .record-board-actions { display: flex; gap: 8px; margin-top: 4px; }
  .record-commit, .record-discard { height: 24px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-body); cursor: pointer; }
  .record-commit { background: var(--ink); color: var(--paper); }
  .record-discard { background: var(--surface); color: var(--ink); }
  .record-columns { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(180px, 1fr); gap: 10px; min-height: 0; overflow: auto; }
  .record-column { display: grid; grid-template-rows: auto minmax(0, 1fr); gap: 6px; min-height: 120px; padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); }
  .record-column.over { border-color: var(--muted); }
  .record-column.unset .record-column-title { color: var(--muted); }
  .record-column-title { display: flex; justify-content: space-between; margin: 0; font: var(--text-12) var(--font-mono); color: var(--ink); }
  .record-column-count { color: var(--muted); }
  .record-cards { margin: 0; padding: 0; list-style: none; display: grid; gap: 6px; align-content: start; overflow-y: auto; }
  .record-card { display: grid; gap: 2px; padding: 6px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); touch-action: none; cursor: grab; }
  .record-card.dragging { opacity: .5; }
  .record-board.dragging { user-select: none; cursor: grabbing; }
  .record-board.dragging .record-card-title { pointer-events: none; }
  .record-card-title { padding: 0; border: 0; background: transparent; color: var(--ink); font: 500 var(--text-13) var(--font-body); text-align: left; cursor: pointer; }
  .record-card-title:hover { text-decoration: underline; }
  .record-card-meta { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-empty { margin: 12px 0 0; color: var(--muted); font: var(--text-13) var(--font-mono); }
</style>

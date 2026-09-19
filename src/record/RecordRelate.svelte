<script>
  // The three actions a record offers beside its fields, each through
  // propose and commit: Link joins it to another record by a relation the
  // vocabulary allows, Merge folds it into a survivor of its kind, and Delete
  // takes it out of every live read while its history stays.
  import { deleteOperation, diffLines, linkOperation, mergeOperation, relationsFrom } from './record-table-state.js'

  let { mode, initialTarget = null, detail, kinds = [], relations = [], tauri, companyId, propose, commit, oncancel, oncommitted } = $props()

  const entity = $derived(detail?.entity)
  const choices = $derived(relationsFrom(relations, entity?.kind, kinds))
  let relation = $state('')
  let targetKind = $state('')
  let search = $state('')
  let candidates = $state([])
  let target = $state(null)
  $effect(() => { target = initialTarget; if (initialTarget) candidates = [initialTarget] })
  let pending = $state(null)
  let error = $state(null)
  let searchVersion = 0

  $effect(() => {
    if (mode === 'merge') targetKind = entity?.kind ?? ''
    else if (mode === 'link' && !relation && choices.length) {
      relation = choices[0].name
      targetKind = choices[0].targets[0] ?? ''
    }
  })

  const targets = $derived(choices.find((choice) => choice.name === relation)?.targets ?? [])

  function chooseRelation(name) {
    relation = name
    const next = choices.find((choice) => choice.name === name)
    if (next && !next.targets.includes(targetKind)) targetKind = next.targets[0] ?? ''
    target = null
    void findCandidates()
  }

  function chooseKind(name) {
    targetKind = name
    target = null
    void findCandidates()
  }

  // The target picker is the kind's own table, searched, without this record.
  async function findCandidates() {
    if (!tauri || !targetKind) return
    const version = ++searchVersion
    error = null
    try {
      const answer = await tauri.invoke('record_query', { companyId, kind: targetKind, search: search.trim() || null, limit: 20, sort: 'updated_at', descending: true })
      if (version !== searchVersion) return
      if (answer?.error) {
        error = answer.error.message
        candidates = []
        return
      }
      candidates = (answer?.page?.rows ?? []).filter((row) => row.id !== entity?.id)
    } catch (failure) {
      if (version !== searchVersion) return
      error = failure?.message ?? String(failure)
    }
  }

  function submitSearch(event) {
    event.preventDefault()
    void findCandidates()
  }

  async function proposeAction(event) {
    event?.preventDefault?.()
    error = null
    let operation
    if (mode === 'delete') operation = deleteOperation(entity.id)
    else if (mode === 'link' && target && relation) operation = linkOperation(entity.id, relation, target.id)
    else if (mode === 'merge' && target) operation = mergeOperation(entity.id, target.id)
    if (!operation) return
    const answer = await propose(operation)
    if (answer?.error) {
      error = answer.error.message ?? 'The record refused the change.'
      return
    }
    pending = { proposal: answer.proposal?.id, lines: diffLines(answer.proposal?.diff), warnings: answer.proposal?.warnings ?? [] }
  }

  async function commitPending() {
    if (!pending?.proposal) return
    const answer = await commit(pending.proposal)
    if (answer?.error) {
      error = answer.error.message ?? 'The commit failed.'
      pending = null
      return
    }
    const opens = mode === 'merge' ? target?.id : mode === 'link' ? entity?.id : null
    pending = null
    oncommitted?.(opens)
  }

  function keydown(event) {
    if (event.key === 'Escape') {
      event.preventDefault()
      if (pending) pending = null
      else oncancel?.()
    }
  }

  const heading = $derived(mode === 'delete' ? `Delete ${entity?.title ?? ''}` : mode === 'merge' ? `Merge ${entity?.title ?? ''} into` : `Link ${entity?.title ?? ''}`)
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form class="record-relate" aria-label={heading} onsubmit={proposeAction} onkeydown={keydown}>
  <p class="record-relate-heading">{heading}</p>
  {#if mode === 'delete'}
    <p class="record-relate-note">The record leaves every table and view. Its history and its identities stay, and a later merge or link cannot name it.</p>
  {:else}
    <div class="record-relate-controls">
      {#if mode === 'link'}
        <label class="record-relate-field">
          <span>Relation</span>
          <select class="record-select" aria-label="Relation" value={relation} onchange={(event) => chooseRelation(event.currentTarget.value)}>
            {#each choices as choice (choice.name)}<option value={choice.name}>{choice.name.replaceAll('_', ' ')}</option>{/each}
          </select>
        </label>
        <label class="record-relate-field">
          <span>Kind</span>
          <select class="record-select" aria-label="Target kind" value={targetKind} onchange={(event) => chooseKind(event.currentTarget.value)}>
            {#each targets as name (name)}<option value={name}>{name.replaceAll('_', ' ')}</option>{/each}
          </select>
        </label>
      {/if}
      <div class="record-relate-search" role="search">
        <input type="search" class="record-relate-input" aria-label="Search {targetKind}" placeholder="Search {targetKind.replaceAll('_', ' ')}" bind:value={search} onkeydown={(event) => { if (event.key === 'Enter') submitSearch(event) }}>
        <button type="button" class="record-discard" onclick={findCandidates}>Find</button>
      </div>
    </div>
    {#if candidates.length}
      <ul class="record-relate-candidates" aria-label="Matches">
        {#each candidates as row (row.id)}
          <li>
            <button type="button" class="record-relate-candidate" aria-pressed={target?.id === row.id} onclick={() => { target = row }}>{row.title}<span class="record-relate-meta">{row.state ? ` · ${row.state}` : ''}</span></button>
          </li>
        {/each}
      </ul>
    {:else if targetKind}
      <p class="record-relate-note">{mode === 'merge' ? 'Find the record to keep by its title.' : 'Find the record by its title.'}</p>
    {/if}
  {/if}
  {#if error}<p class="record-relate-error" role="alert">{error}</p>{/if}
  {#if pending}
    <div class="record-relate-diff" role="group" aria-label="Proposed {mode}">
      {#each pending.lines as line (line)}<p class="record-diff-line">{line}</p>{/each}
      {#each pending.warnings as warning (warning)}<p class="record-diff-warning">{warning}</p>{/each}
      <div class="record-relate-actions">
        <button type="button" class="record-commit" onclick={commitPending}>Commit</button>
        <button type="button" class="record-discard" onclick={() => { pending = null }}>Discard</button>
      </div>
    </div>
  {:else}
    <div class="record-relate-actions">
      <button type="submit" class="record-commit" disabled={mode !== 'delete' && !target}>Propose</button>
      <button type="button" class="record-discard" onclick={() => oncancel?.()}>Cancel</button>
    </div>
  {/if}
</form>

<style>
  .record-relate { display: grid; gap: 10px; align-content: start; min-height: 0; overflow-y: auto; padding-top: 12px; }
  .record-relate-heading { margin: 0; font: 600 var(--text-15)/1.3 var(--font-human); }
  .record-relate-note { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-relate-controls { display: grid; gap: 8px; }
  .record-relate-field { display: flex; align-items: center; gap: 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-select { height: 24px; max-width: 220px; padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); }
  .record-relate-search { display: flex; gap: 8px; }
  .record-relate-input { flex: 1; height: 26px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  .record-relate-input:focus { outline: none; border-color: var(--muted); }
  .record-relate-candidates { margin: 0; padding: 0; list-style: none; display: grid; }
  .record-relate-candidates li + li { border-top: 1px solid var(--border); }
  .record-relate-candidate { width: 100%; min-height: 28px; padding: 0 6px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); text-align: left; font: var(--text-13) var(--font-human); cursor: pointer; }
  .record-relate-candidate:hover, .record-relate-candidate[aria-pressed="true"] { background: var(--faint); }
  .record-relate-meta { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-relate-diff { display: grid; gap: 4px; padding-top: 8px; border-top: 1px solid var(--border); }
  .record-diff-line { margin: 0; font: var(--text-12) var(--font-mono); }
  .record-diff-warning { margin: 0; color: var(--ochre); font: var(--text-12) var(--font-mono); }
  .record-relate-error { margin: 0; color: var(--oxide); font: var(--text-12) var(--font-mono); }
  .record-relate-actions { display: flex; gap: 8px; }
  .record-commit, .record-discard { height: 26px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-human); cursor: pointer; }
  .record-commit { background: var(--ink); color: var(--paper); }
  .record-commit:disabled { background: var(--faint); color: var(--muted); cursor: default; }
  .record-discard { background: var(--surface); color: var(--ink); }
</style>

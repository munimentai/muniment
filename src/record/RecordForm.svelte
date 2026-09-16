<script>
  // A new record: one input per property, the required ones first, generated
  // from the kind. Submit proposes, the diff shows, Commit applies it.
  import { coerceInput, createOperation, diffLines, formFields } from './record-table-state.js'

  let { kind, propose, commit, oncreated, oncancel } = $props()

  const fields = $derived(formFields(kind))
  let values = $state({})
  let pending = $state(null)
  let error = $state(null)

  const ready = $derived(fields.filter((field) => field.required).every((field) => (values[field.key] ?? '').toString().trim() !== ''))

  async function submit(event) {
    event.preventDefault()
    error = null
    const data = {}
    for (const field of fields) {
      const value = coerceInput(values[field.key], field)
      if (value !== null) data[field.key] = value
    }
    const answer = await propose(createOperation(kind, data))
    if (answer?.error) {
      error = answer.error.prompt ?? answer.error.message ?? 'The record refused the change.'
      return
    }
    pending = { proposal: answer.proposal?.id, lines: diffLines(answer.proposal?.diff), warnings: answer.proposal?.warnings ?? [], id: answer.proposal?.diff?.after?.id }
  }

  async function commitPending() {
    if (!pending?.proposal) return
    const answer = await commit(pending.proposal)
    if (answer?.error) {
      error = answer.error.message ?? 'The commit failed.'
      pending = null
      return
    }
    const id = answer?.result?.entity_ids?.[0] ?? pending.id
    pending = null
    oncreated?.(id)
  }

  function keydown(event) {
    if (event.key === 'Escape') {
      event.preventDefault()
      if (pending) pending = null
      else oncancel?.()
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form class="record-form" aria-label="New {kind?.name}" onsubmit={submit} onkeydown={keydown}>
  {#each fields as field (field.key)}
    <label class="record-field">
      <span class="record-field-label">{field.label}{field.required ? ' *' : ''}</span>
      {#if field.enum}
        <select class="record-input mono" bind:value={values[field.key]} required={field.required}>
          <option value="">choose</option>
          {#each field.enum as option (option)}<option value={option}>{option}</option>{/each}
        </select>
      {:else if field.type === 'date'}
        <!-- The webview's date field edits by segment and settles the value on change, so the change event feeds the form too. -->
        <input class="record-input mono" type="date" bind:value={values[field.key]} onchange={(event) => { values[field.key] = event.currentTarget.value }} required={field.required}>
      {:else if field.type === 'number' || field.type === 'integer'}
        <input class="record-input mono" type="number" step={field.type === 'integer' ? 1 : 'any'} bind:value={values[field.key]} required={field.required}>
      {:else if field.type === 'boolean'}
        <select class="record-input mono" bind:value={values[field.key]}><option value="">choose</option><option value="yes">yes</option><option value="no">no</option></select>
      {:else}
        <input class="record-input" class:mono={field.mono} type="text" bind:value={values[field.key]} required={field.required}>
      {/if}
    </label>
  {/each}
  {#if error}<p class="record-form-error" role="alert">{error}</p>{/if}
  {#if pending}
    <div class="record-form-diff" role="group" aria-label="Proposed record">
      {#each pending.lines as line (line)}<p class="record-diff-line">{line}</p>{/each}
      {#each pending.warnings as warning (warning)}<p class="record-diff-warning">{warning}</p>{/each}
      <div class="record-form-actions">
        <button type="button" class="record-commit" onclick={commitPending}>Commit</button>
        <button type="button" class="record-discard" onclick={() => { pending = null }}>Discard</button>
      </div>
    </div>
  {:else}
    <div class="record-form-actions">
      <button type="submit" class="record-commit" disabled={!ready}>Propose</button>
      <button type="button" class="record-discard" onclick={() => oncancel?.()}>Cancel</button>
    </div>
  {/if}
</form>

<style>
  .record-form { display: grid; gap: 10px; align-content: start; min-height: 0; overflow-y: auto; padding-top: 12px; }
  .record-field { display: grid; gap: 4px; }
  .record-field-label { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-input { height: 28px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-body); }
  .record-input.mono { font: var(--text-12) var(--font-mono); }
  .record-input:focus { outline: none; border-color: var(--muted); }
  .record-form-error { margin: 0; color: var(--oxide); font: var(--text-12) var(--font-mono); }
  .record-form-diff { display: grid; gap: 4px; padding-top: 8px; border-top: 1px solid var(--border); }
  .record-diff-line { margin: 0; font: var(--text-12) var(--font-mono); }
  .record-diff-warning { margin: 0; color: var(--ochre); font: var(--text-12) var(--font-mono); }
  .record-form-actions { display: flex; gap: 8px; }
  .record-commit, .record-discard { height: 26px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-body); cursor: pointer; }
  .record-commit { background: var(--ink); color: var(--paper); }
  .record-commit:disabled { background: var(--faint); color: var(--muted); cursor: default; }
  .record-discard { background: var(--surface); color: var(--ink); }
</style>

<script>
  // Import: a CSV file lands on the open kind. The dialog picks the file,
  // the runtime describes its columns, this form proposes a `mapping` record
  // onto the kind's properties with one identity column, Commit approves it,
  // and the run pages the rows through propose and commit until it is done.
  // Every row the mapping could not place lists with its reason.
  import { open } from '@tauri-apps/plugin-dialog'
  import { diffLines } from './record-table-state.js'
  import { accumulateRun, identityOptions, importErrorLine, mappedCount, mappingData, runSummaryLines, suggestFields, suggestIdentity, targetProperties } from './record-import-state.js'

  let { tauri, companyId, kind, mapping: existingMapping = null, propose, commit, oncancel, ondone } = $props()

  // 'picking' | 'mapping' | 'proposed' | 'running' | 'done' | 'failed'
  let step = $state(existingMapping ? 'running' : 'picking')
  let description = $state(null)
  let fields = $state({})
  let identity = $state('')
  let pending = $state(null)
  let mappingId = $state(existingMapping)
  let run = $state(null)
  let progress = $state(null)
  let error = $state(null)
  const properties = $derived(targetProperties(kind))
  const identities = $derived(identityOptions(description))
  const mapped = $derived(mappedCount(fields))
  const summary = $derived(runSummaryLines(run))

  $effect(() => {
    if (step === 'picking') void pick()
    else if (step === 'running' && !run && !progress) void runMapping()
  })

  async function pick() {
    let picked
    try {
      picked = await open({ multiple: false, directory: false, filters: [{ name: 'CSV', extensions: ['csv', 'tsv', 'txt'] }] })
    } catch {
      picked = null
    }
    const path = Array.isArray(picked) ? picked[0] : picked
    if (typeof path !== 'string' || !path) {
      oncancel?.()
      return
    }
    let answer
    try {
      answer = await tauri.invoke('reader_describe', { companyId, source: 'csv', object: path })
    } catch (failure) {
      answer = { error: { message: importErrorLine(failure) } }
    }
    if (answer?.error || !answer?.description) {
      error = importErrorLine(answer ?? 'The file could not be read')
      step = 'failed'
      return
    }
    description = answer.description
    fields = suggestFields(description, kind)
    identity = suggestIdentity(description)
    step = 'mapping'
  }

  async function proposeMapping(event) {
    event?.preventDefault?.()
    if (!description || mapped === 0) return
    error = null
    const answer = await propose({ op: 'create', kind: 'mapping', data: mappingData(description, kind, fields, identity) })
    if (answer?.error) {
      error = importErrorLine(answer)
      return
    }
    pending = { proposal: answer.proposal?.id, lines: diffLines(answer.proposal?.diff), warnings: answer.proposal?.warnings ?? [] }
    step = 'proposed'
  }

  async function commitMapping() {
    if (!pending?.proposal) return
    const answer = await commit(pending.proposal)
    if (answer?.error) {
      error = importErrorLine(answer)
      return
    }
    mappingId = answer?.result?.entity_ids?.[0] ?? null
    pending = null
    if (!mappingId) {
      error = 'The mapping committed without an id.'
      step = 'failed'
      return
    }
    step = 'running'
  }

  // One call lands rows for its budget and answers with its offset. The
  // loop runs until the runtime says it reached the end of the file.
  async function runMapping() {
    error = null
    run = null
    let offset = 0
    let total = null
    progress = { offset: 0, total: null }
    for (;;) {
      let answer
      try {
        answer = await tauri.invoke('reader_run', { companyId, mapping: mappingId, offset })
      } catch (failure) {
        answer = { error: { message: importErrorLine(failure) } }
      }
      if (answer?.error || !answer?.run) {
        error = importErrorLine(answer ?? 'The run did not answer')
        progress = null
        step = 'failed'
        return
      }
      total = accumulateRun(total, answer.run)
      offset = answer.run.next_offset ?? offset
      progress = { offset, total: answer.run.total }
      if (answer.run.done || answer.run.next_offset === undefined) break
    }
    run = total
    progress = null
    step = 'done'
  }

  function discard() {
    pending = null
    step = 'mapping'
  }

  function keydown(event) {
    if (event.key === 'Escape') {
      event.preventDefault()
      if (step === 'proposed') discard()
      else if (step === 'done' || step === 'failed') ondone?.(run)
      else oncancel?.()
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="record-import" aria-label="Import {kind?.name ?? ''}" onkeydown={keydown}>
  {#if step === 'picking'}
    <p class="record-import-state">Choose a CSV file</p>
  {:else if step === 'failed'}
    <p class="record-import-error" role="alert">{error}</p>
    <div class="record-import-actions">
      <button type="button" class="record-discard" onclick={() => ondone?.(run)}>Close</button>
    </div>
  {:else if step === 'mapping' || step === 'proposed'}
    <form class="record-import-form" aria-label="Map {description?.label ?? 'the file'}" onsubmit={proposeMapping}>
      <p class="record-import-file">{description.label} · {description.rows} {description.rows === 1 ? 'row' : 'rows'}</p>
      <table class="record-import-fields" aria-label="Columns">
        <thead>
          <tr><th scope="col">column</th><th scope="col">reads as</th><th scope="col">fills</th></tr>
        </thead>
        <tbody>
          {#each description.fields as field (field.name)}
            <tr>
              <th scope="row" class="record-import-column">{field.name}</th>
              <td class="record-import-sample">{field.guess}{field.samples.length ? ` · ${field.samples.join(', ')}` : ''}</td>
              <td>
                <select class="record-select" aria-label="Property for {field.name}" value={fields[field.name] ?? ''} disabled={step === 'proposed'} onchange={(event) => { fields = { ...fields, [field.name]: event.currentTarget.value } }}>
                  <option value="">skip</option>
                  {#each properties as property (property)}<option value={property}>{property.startsWith('x_') ? `${property.slice(2)} (own)` : property}</option>{/each}
                </select>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <label class="record-import-identity">
        <span>Key each row on</span>
        <select class="record-select" aria-label="Identity" value={identity} disabled={step === 'proposed'} onchange={(event) => { identity = event.currentTarget.value }}>
          {#each identities as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
        </select>
      </label>
      <p class="record-import-note">A second run of the same file updates the rows it keyed and adds none twice.</p>
      {#if error}<p class="record-import-error" role="alert">{error}</p>{/if}
      {#if step === 'proposed' && pending}
        <div class="record-import-diff" role="group" aria-label="Proposed mapping">
          {#each pending.lines as line (line)}<p class="record-diff-line">{line}</p>{/each}
          {#each pending.warnings as warning (warning)}<p class="record-diff-warning">{warning}</p>{/each}
          <div class="record-import-actions">
            <button type="button" class="record-commit" onclick={commitMapping}>Commit and run</button>
            <button type="button" class="record-discard" onclick={discard}>Discard</button>
          </div>
        </div>
      {:else}
        <div class="record-import-actions">
          <button type="submit" class="record-commit" disabled={mapped === 0}>Propose mapping</button>
          <button type="button" class="record-discard" onclick={() => oncancel?.()}>Cancel</button>
        </div>
      {/if}
    </form>
  {:else if step === 'running'}
    <p class="record-import-state" role="status">{progress?.total ? `${Math.min(progress.offset, progress.total)} of ${progress.total} rows` : 'Reading the file'}</p>
  {:else if step === 'done'}
    <div class="record-import-result" role="group" aria-label="Import result">
      {#each summary as line (line)}<p class="record-import-line">{line}</p>{/each}
      {#if run?.queue?.length}
        <table class="record-import-queue" aria-label="Rows not placed">
          <thead>
            <tr><th scope="col">row</th><th scope="col">title</th><th scope="col">reason</th></tr>
          </thead>
          <tbody>
            {#each run.queue as entry (entry.row)}
              <tr><td>{entry.row}</td><td class="record-import-title">{entry.title}</td><td>{entry.reason}</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <div class="record-import-actions">
        <button type="button" class="record-commit" onclick={() => ondone?.(run)}>Done</button>
        <button type="button" class="record-discard" onclick={() => { run = null; progress = null; step = 'running' }}>Run again</button>
      </div>
    </div>
  {/if}
</section>

<style>
  .record-import { display: grid; align-content: start; gap: 10px; min-height: 0; overflow-y: auto; padding-top: 12px; }
  .record-import-state, .record-import-note, .record-import-file { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-import-file { color: var(--ink); }
  .record-import-form, .record-import-result { display: grid; gap: 10px; }
  .record-import-fields, .record-import-queue { width: 100%; border-collapse: collapse; font: var(--text-12) var(--font-mono); }
  .record-import-fields th, .record-import-queue th { padding: 0 8px 6px 0; text-align: left; color: var(--muted); font-weight: 400; border-bottom: 1px solid var(--border); }
  .record-import-fields td, .record-import-queue td, .record-import-fields th[scope="row"] { height: 28px; padding: 0 8px 0 0; border-bottom: 1px solid var(--border); vertical-align: middle; }
  .record-import-column { color: var(--ink); font-weight: 500; white-space: nowrap; }
  .record-import-sample { color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 220px; }
  .record-import-title { font: var(--text-12) var(--font-body); }
  .record-select { height: 24px; max-width: 200px; padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); }
  .record-import-identity { display: flex; align-items: center; gap: 8px; font: var(--text-12) var(--font-mono); color: var(--ink); }
  .record-import-diff { display: grid; gap: 4px; padding-top: 8px; border-top: 1px solid var(--border); }
  .record-diff-line { margin: 0; font: var(--text-12) var(--font-mono); }
  .record-diff-warning { margin: 0; color: var(--ochre); font: var(--text-12) var(--font-mono); }
  .record-import-error { margin: 0; color: var(--oxide); font: var(--text-12) var(--font-mono); }
  .record-import-line { margin: 0; font: var(--text-12) var(--font-mono); }
  .record-import-actions { display: flex; gap: 8px; }
  .record-commit, .record-discard { height: 26px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-body); cursor: pointer; }
  .record-commit { background: var(--ink); color: var(--paper); }
  .record-commit:disabled { background: var(--faint); color: var(--muted); cursor: default; }
  .record-discard { background: var(--surface); color: var(--ink); }
</style>

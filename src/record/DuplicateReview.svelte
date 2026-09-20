<script>
  import RecordView from './RecordView.svelte'
  let { records, onmerge } = $props()
  let keep = $state('')
  let merge = $state('')
  const survivor = $derived(records.find((record) => record.entity.id === keep))
  const duplicate = $derived(records.find((record) => record.entity.id === merge && merge !== keep))
  const fields = $derived([...new Set(records.flatMap((record) => Object.keys(record.entity.data ?? {})))])
  const values = (record) => ({ Title: record.entity.title, State: record.entity.state, Description: record.entity.body_text, ...record.entity.data })
  const text = (value) => value == null || value === '' ? 'Not set' : typeof value === 'object' ? JSON.stringify(value) : String(value)
  const differences = $derived(['Title', 'State', 'Description', ...fields].filter((field) => new Set(records.map((record) => text(values(record)[field]))).size > 1))
  function choose(id) {
    keep = id
    merge = records.length === 2 ? records.find((record) => record.entity.id !== id).entity.id : ''
  }
</script>

<section class="comparison" aria-label="Compare possible duplicates">
  <p>Choose the record to keep, then preview one merge.</p>
  {#if !differences.length}<p class="muted">These records have matching fields. Check their sources below.</p>{/if}
  <div class="records">
    {#each records as detail, index (detail.entity.id)}
      <article aria-label={`Record ${index + 1}: ${detail.entity.title}`}>
        <header><span class="muted">Record {index + 1}</span><h5>{detail.entity.title}</h5></header>
        <button type="button" aria-pressed={keep === detail.entity.id} onclick={() => choose(detail.entity.id)}>Keep this record</button>
        {#if differences.length}
          <dl aria-label="Differing fields">
            {#each differences as field}<div><dt>{field.replaceAll('_', ' ')}</dt><dd>{text(values(detail)[field])}</dd></div>{/each}
          </dl>
        {/if}
        <details><summary>Sources and all fields</summary><RecordView {detail} /></details>
      </article>
    {/each}
  </div>
  {#if survivor && records.length > 2}
    <label>Record to merge
      <select bind:value={merge}>
        <option value="">Choose a duplicate</option>
        {#each records as detail, index (detail.entity.id)}
          {#if detail.entity.id !== keep}<option value={detail.entity.id}>Record {index + 1}: {detail.entity.title}</option>{/if}
        {/each}
      </select>
    </label>
  {/if}
  {#if survivor && duplicate}<p class="muted">Record {records.indexOf(duplicate) + 1} merges into record {records.indexOf(survivor) + 1}. Review before committing.</p>{/if}
  <button class="preview" type="button" disabled={!survivor || !duplicate} onclick={() => onmerge?.(duplicate, survivor.entity)}>Preview merge</button>
</section>

<style>
  .comparison { container-type: inline-size; min-width: 0; display: grid; gap: 12px; }
  p { margin: 0; font: var(--text-13)/1.5 var(--font-human); }
  .muted { color: var(--muted); }
  .records { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  @container (max-width: 500px) { .records { grid-template-columns: 1fr; } }
  article { min-width: 0; display: grid; align-content: start; gap: 10px; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-control); overflow-wrap: anywhere; }
  header span { font: var(--text-12) var(--font-mono); }
  h5 { margin: 4px 0 0; font: 600 var(--text-13) var(--font-human); }
  button, select { box-sizing: border-box; max-width: 100%; padding: 6px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  button { cursor: pointer; white-space: normal; }
  button[aria-pressed="true"] { background: var(--ink); color: var(--paper); }
  button:disabled { color: var(--muted); cursor: default; }
  .preview { justify-self: start; }
  label { display: grid; gap: 6px; min-width: 0; font-size: var(--text-13); }
  select { width: 100%; min-width: 0; }
  dl { margin: 0; display: grid; gap: 8px; font: var(--text-12)/1.4 var(--font-mono); }
  dt { color: var(--muted); } dd { margin: 2px 0 0; }
  details { min-width: 0; overflow-wrap: anywhere; } summary { cursor: pointer; font-size: var(--text-12); }
</style>

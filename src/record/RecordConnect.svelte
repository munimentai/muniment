<script>
  // The record panel's first screen for a company that holds no records:
  // what the company record is, and the sources that fill it, each under
  // its own mark. A source opens the import on that source, and the kind
  // follows from the object the user picks there.
  import RecordSources from './RecordSources.svelte'

  let { company = null, onconnect } = $props()
</script>

<section class="record-connect" aria-label="Connect your data" data-testid="record-connect">
  <h3 class="record-connect-title">Connect your data</h3>
  <p class="record-connect-lede">{company?.name ?? 'This company'} has no records yet. The company record takes over from the systems of record your tools hold. Each source below reads its objects in, keyed on the source's own ids, so a second run updates what changed and never duplicates a row. The agent reads and writes the record from the thread.</p>
  <RecordSources {onconnect} />
  <p class="record-connect-foot">Every read runs on this machine, and a credential stays in its keychain. Nothing here writes back to a source.</p>
</section>

<style>
  .record-connect { display: grid; align-content: start; gap: 14px; min-height: 0; overflow-y: auto; padding-top: 14px; }
  .record-connect-title { margin: 0; font: 600 var(--text-15)/1.3 var(--font-human); }
  .record-connect-lede, .record-connect-foot { margin: 0; max-width: 62ch; color: var(--ink); font: var(--text-13)/1.5 var(--font-human); }
  .record-connect-foot { color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); }
</style>

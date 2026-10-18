<script>
  // A failure inline: its first sentence, then everything the host or core
  // said about it behind Show log, so the root error is one click away on
  // every failure.
  import { failureLog, failureLine } from './failure-note.js'

  let { failure = '', detail = '', fallback = 'Something went wrong.', role = 'alert' } = $props()
  const line = $derived(failureLine(failure, fallback))
  const log = $derived(failureLog(failure, detail))
</script>

<div class="failure-note" {role}>
  <p>{line}</p>
  {#if log.length}<details><summary>Show log</summary>{#each log as entry, index (index)}<pre>{entry}</pre>{/each}</details>{/if}
</div>

<style>
  .failure-note { display: grid; gap: 4px; min-width: 0; }
  p { margin: 0; color: var(--ink); font-size: var(--text-13); }
  details { color: var(--muted); font-size: var(--text-12); }
  summary { cursor: pointer; width: max-content; }
  pre { margin: 4px 0 0; padding: 8px; overflow-x: auto; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); font: var(--text-12) var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
</style>

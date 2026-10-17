<script>
  // The composer's note for a message queued while the open thread's owner
  // is offline: the queued line, Cancel, which drops the message before it
  // leaves this computer, and Move here for an agent whose machine has been
  // away a day or more. A replica thread whose history is still travelling
  // shows that line instead.
  import Button from '../lib/ui/Button.svelte'
  import { queuedLine } from './fleet.js'

  let { op = null, history = '', moveSuggested = false, error = '', oncancel = () => {}, onmove = () => {} } = $props()
  let busy = $state('')

  async function act(name, action) {
    busy = name
    try { await action() } finally { busy = '' }
  }
</script>

{#if op || history || moveSuggested}
  <section class="queued-notice" aria-live="polite">
    {#if op}<p class="queued-line">{queuedLine(op)}</p>{:else if history}<p class="queued-line">{history}</p>{/if}
    {#if op || moveSuggested}
      <div class="queued-actions">
        {#if op}<Button disabled={!!busy} onclick={() => act('cancel', () => oncancel(op))}>Cancel</Button>{/if}
        {#if moveSuggested}<Button disabled={!!busy} onclick={() => act('move', onmove)}>Move here</Button>{/if}
      </div>
    {/if}
    {#if error}<p class="queued-error" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  .queued-notice { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 12px; margin: 6px 0 8px; }
  .queued-line { flex: 1 1 240px; margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); line-height: var(--leading-body); }
  .queued-actions { display: flex; flex: none; gap: 6px; }
  .queued-error { flex: 1 1 100%; margin: 0; color: var(--ink); font-size: var(--text-12); }
</style>

<script>
  // Core pauses every member's routines once no client has used the swarm
  // for 14 days. The next visit asks here to resume them, and one resume
  // reaches every member.
  import Button from '../lib/ui/Button.svelte'
  import { agentRequest } from './agent-runtime.js'

  let { tauri, runtime, pausedAt = null, onresumed = () => {} } = $props()
  let busy = $state(false)
  let error = $state('')
  let hidden = $state(false)
  const since = $derived(pausedAt ? new Date(pausedAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : '')

  async function resume() {
    busy = true
    error = ''
    try {
      await agentRequest(tauri, runtime, 'swarm.resume', {})
      onresumed()
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }
</script>

{#if !hidden}
  <section class="absence" aria-label="Routines paused">
    <p><strong>Routines paused{since ? ` since ${since}` : ''}.</strong> No one used Muniment for 14 days, so agents stopped running on their own.</p>
    <div class="actions">
      <Button variant="primary" disabled={busy} onclick={resume}>{busy ? 'Resuming…' : 'Resume routines'}</Button>
      <Button disabled={busy} onclick={() => { hidden = true }}>Not now</Button>
    </div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  .absence { display: grid; gap: 8px; margin: 4px 8px 8px; padding: 10px; border: 1px solid var(--ochre); border-radius: var(--radius-control); background: var(--surface); }
  p { margin: 0; font-size: var(--text-12); line-height: 1.5; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .error { color: var(--oxide); }
</style>

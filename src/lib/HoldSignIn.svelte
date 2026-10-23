<script>
  // Who holds one shared sign-in, the machine that refreshes it, in one mono
  // line. Hold sign-in here asks core's `fleet.hold_sign_in` on `runtime`,
  // the machine the page shows, to move the refresher there, which answers
  // `held` at once or `requested` while the holder hands it over.
  import Button from './ui/Button.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { holdLine } from './fleet-accounts.js'

  let { tauri, account, onmoved, runtime = 'local' } = $props()
  let requested = $state(false)
  let pending = $state(false)
  let error = $state('')

  async function hold() {
    pending = true
    error = ''
    try {
      const answer = await agentRequest(tauri, runtime, 'fleet.hold_sign_in', { account: account.name })
      requested = true
      onmoved?.(answer?.status)
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      pending = false
    }
  }
</script>

<span class="hold" data-hold={account.name}>
  <span class="record">{holdLine(account, requested)}</span>
  {#if !account.this_machine && !requested}<Button disabled={pending} onclick={hold}>Hold sign-in here</Button>{/if}
  {#if error}<span class="error" role="alert">{error}</span>{/if}
</span>

<style>
  /* One row tall with or without the control, so cards side by side keep their lines level. */
  .hold { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; min-height: 32px; }
  .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .error { flex-basis: 100%; color: var(--ink); font-size: var(--text-12); overflow-wrap: anywhere; }
</style>

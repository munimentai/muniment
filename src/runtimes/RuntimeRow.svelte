<script>
  // One runtime on the Runtimes page: its kind, version and state, the in-use
  // mark or Switch, Update when this device may update an older runtime, and
  // Rename and Remove for a paired runtime. Remove asks once and names it.
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { kindIcon, kindLabel, statusHint, statusLabel, statusTone, versionLabel } from './runtimes.js'

  let { runtime, busy = false, switchDisabled = false, onselect, onrename, onremove, onupdate } = $props()
  let mode = $state('view')
  let draft = $state('')

  async function rename(event) {
    event.preventDefault()
    if (await onrename(draft.trim())) mode = 'view'
  }

  async function remove() {
    if (await onremove()) mode = 'view'
  }
</script>

{#if mode === 'rename'}
  <form class="runtime-form" aria-label="Rename {runtime.name}" onsubmit={rename}>
    <input type="text" aria-label="Runtime name" maxlength="80" bind:value={draft}>
    <Button type="submit" disabled={busy || !draft.trim()}>Save</Button>
    <button type="button" class="quiet" onclick={() => { mode = 'view' }}>Cancel</button>
  </form>
{:else if mode === 'remove'}
  <Row title="Remove {runtime.name}?" description="This computer forgets its access. Its threads stay on it.">
    <Button disabled={busy} onclick={remove}>Remove</Button>
    <button type="button" class="quiet" onclick={() => { mode = 'view' }}>Keep</button>
  </Row>
{:else}
  <Row title={runtime.name} meta={versionLabel(runtime)} label={runtime.name}>
    {#snippet lead()}<span class="runtime-mark" aria-hidden="true"><LucideIcon name={kindIcon(runtime)} size={16} variant="action" /></span>{/snippet}
    {#snippet detail()}
      <p class="runtime-facts">
        {#if !runtime.local}<span>{kindLabel(runtime)}</span>{/if}
        {#if runtime.sshHost}<span class="record">ssh {runtime.sshHost}</span>{/if}
        <span class="runtime-state" data-tone={statusTone(runtime)}>{statusLabel(runtime)}</span>
      </p>
      {#if statusHint(runtime)}<p class="runtime-hint">{statusHint(runtime)}</p>{/if}
    {/snippet}
    {#if runtime.updateOffered}<Button icon="arrow-down" disabled={busy} aria-label="Update {runtime.name}" onclick={onupdate}>Update</Button>{/if}
    {#if runtime.active}
      <span class="in-use"><LucideIcon name="check" size={14} variant="action" />In use</span>
    {:else}
      <Button disabled={busy || switchDisabled} aria-label="Switch to {runtime.name}" onclick={onselect}>Switch</Button>
    {/if}
    {#if !runtime.local}
      <button type="button" class="quiet" disabled={busy} aria-label="Rename {runtime.name}" onclick={() => { draft = runtime.name; mode = 'rename' }}>Rename</button>
      <button type="button" class="quiet" disabled={busy} aria-label="Remove {runtime.name}" onclick={() => { mode = 'remove' }}>Remove</button>
    {/if}
  </Row>
{/if}

<style>
  .runtime-mark { display: grid; place-items: center; width: 32px; height: 32px; box-sizing: border-box; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); }
  .runtime-facts { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; margin: 0; color: var(--muted); font-size: var(--text-13); }
  .record { font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .runtime-state[data-tone="ink"] { color: var(--ink); }
  .runtime-state[data-tone="caution"] { color: var(--ochre); }
  .runtime-state[data-tone="critical"] { color: var(--oxide); }
  .runtime-hint { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .in-use { display: inline-flex; align-items: center; gap: 6px; min-height: 32px; padding: 0 10px; color: var(--muted); font-size: var(--text-13); }
  .quiet { background: transparent; }
  .runtime-form { display: flex; align-items: center; gap: 8px; padding: 10px 0; }
  .runtime-form input { flex: 1; min-width: 0; }
</style>

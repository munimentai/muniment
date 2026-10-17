<script>
  // The fleet member that owns an item: its shape icon and "on Studio" in
  // mono, with an ochre slashed globe while that member does not answer.
  // `compact` keeps the icons and gives the name to screen readers alone.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { kindIcon } from './runtimes.js'
  let { machine, prefix = 'on', compact = false } = $props()
  const offline = $derived(machine?.online === false)
</script>

{#if machine}
  <span class="owner-chip" data-offline={offline ? 'true' : undefined}>
    <LucideIcon name={kindIcon(machine)} size={12} />
    <span class={compact ? 'visually-hidden' : 'owner-name'}>{prefix ? `${prefix} ` : ''}{machine.name}</span>
    {#if offline}<span class="owner-offline"><LucideIcon name="globe-off" size={12} variant="action" /><span class="visually-hidden">, offline</span></span>{/if}
  </span>
{/if}

<style>
  .owner-chip { display: inline-flex; flex: 0 1 auto; align-items: center; gap: 4px; min-width: 0; color: var(--muted); font: var(--text-provenance) var(--font-mono); line-height: 16px; white-space: nowrap; }
  .owner-chip :global(.lucide) { flex: none; color: var(--muted); }
  .owner-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .owner-offline { display: inline-flex; flex: none; }
  .owner-chip .owner-offline :global(.lucide) { color: var(--ochre); }
</style>

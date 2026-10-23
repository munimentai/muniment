<script>
  // The fleet member that owns an item: its shape icon and "on Studio" in
  // mono, with a warning slashed globe while that member does not answer.
  // `compact` keeps the icons and gives the name to screen readers alone.
  // `icon`, when given, draws in place of the shape icon.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { kindIcon, machineName } from './runtimes.js'
  let { machine, prefix = 'on', compact = false, icon = undefined } = $props()
  const name = $derived(machine ? machineName(machine) : '')
  const offline = $derived(machine?.online === false)
</script>

{#if machine}
  <span class="owner-chip" data-offline={offline ? 'true' : undefined}>
    {#if icon}{@render icon()}{:else}<LucideIcon name={kindIcon(machine)} size={12} />{/if}
    {#if compact}<span class="visually-hidden">{prefix ? `${prefix} ` : ''}{name}</span>{:else}<span class="owner-name">{prefix ? `${prefix} ` : ''}{name}</span>{/if}
    {#if offline}<span class="owner-offline"><LucideIcon name="globe-off" size={12} variant="action" /><span class="visually-hidden">, offline</span></span>{/if}
  </span>
{/if}

<style>
  .owner-chip { display: inline-flex; flex: 0 1 auto; align-items: center; gap: 4px; min-width: 0; color: var(--muted); font: var(--text-12) var(--font-human); line-height: 16px; white-space: nowrap; }
  .owner-chip :global(.lucide) { flex: none; color: var(--muted); }
  .owner-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .owner-offline { display: inline-flex; flex: none; }
  .owner-chip .owner-offline :global(.lucide) { color: var(--warning); }
</style>

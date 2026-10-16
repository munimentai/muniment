<script>
  // The machine an agent or run lives on: its form-factor icon and name.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { kindIcon } from '../runtimes/runtimes.js'
  let { machine, named = true } = $props()
</script>

<span class="machine-mark"><LucideIcon name={kindIcon(machine)} size={12} />{#if named}<span class="machine-name">{machine?.name}</span>{:else}<span class="visually-hidden">{machine?.name}</span>{/if}</span>

<style>
  .machine-mark { display: inline-flex; flex: 0 1 auto; align-items: center; gap: 4px; min-width: 0; color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  .machine-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
</style>

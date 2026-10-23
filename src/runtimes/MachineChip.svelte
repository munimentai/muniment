<script>
  // A machine named in a list: its shape icon in its stoplight light, green
  // while it answers, yellow while it connects, red when it does not, then its
  // name. The state shows on hover and follows the name for screen readers.
  // A machine the runtime list does not know shows its name alone.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { machineLight } from '../lib/status-tone.js'
  import { kindIcon, machineState, runtimeFor } from './runtimes.js'
  let { machine, runtimeList = null } = $props()
  const runtime = $derived(runtimeFor(runtimeList, machine))
  const light = $derived(machineLight(runtime))
  const state = $derived(runtime ? machineState(runtime) : '')
</script>

<span class="machine-chip" data-light={light} data-tooltip={state || undefined}>{#if runtime}<span class="mark"><LucideIcon name={kindIcon(runtime)} variant="action" size={12} /></span>{/if}<span class="name">{machine?.name}</span>{#if state}<span class="visually-hidden">, {state}</span>{/if}</span>

<style>
  .machine-chip { position: relative; display: inline-flex; flex: none; align-items: center; gap: 4px; max-width: 100%; padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-human); line-height: 18px; white-space: nowrap; }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .mark { display: inline-flex; color: var(--muted); }
  [data-light="green"] .mark { color: var(--success); }
  [data-light="yellow"] .mark { color: var(--warning); }
  [data-light="red"] .mark { color: var(--danger); }
  .machine-chip[data-tooltip]::after { content: attr(data-tooltip); position: absolute; z-index: 4; left: -1px; bottom: calc(100% + 6px); padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .machine-chip[data-tooltip]:hover::after { opacity: 1; transition-delay: 350ms; }
</style>

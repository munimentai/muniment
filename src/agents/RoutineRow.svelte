<script>
  // One routine in a list: its trigger's icon, its name, when it runs in
  // local time with no time zone, a failed tool check and a small switch.
  // The name opens the routine through `onedit`, and the switch calls
  // `ontoggle` with the routine's new state.
  import Toggle from '../lib/Toggle.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { pollLabel, routineIcon, routineWhen } from './routine-labels.js'

  let { routine, disabled = false, editLabel = undefined, onedit, ontoggle } = $props()
</script>

<div class="routine" role="group" aria-label={routine.name}>
  <span class="icon"><LucideIcon name={routineIcon(routine.trigger)} variant="action" size={14} /></span>
  <div class="text">
    <button type="button" class="name" aria-label={editLabel} {disabled} onclick={() => onedit(routine)}>{routine.name}</button>
    <span class="when">{routineWhen(routine)}</span>
    {#if routine.last_poll?.error}<span class="poll-error">{pollLabel(routine)}</span>{/if}
  </div>
  <Toggle size="small" checked={routine.enabled} label={`${routine.name} on`} {disabled} onchange={(checked) => ontoggle(routine, checked)} />
</div>

<style>
  .routine { display: flex; align-items: flex-start; gap: 8px; }
  .icon { flex: none; display: inline-flex; padding-top: 4px; color: var(--muted); }
  .text { display: grid; flex: 1; gap: 1px; min-width: 0; }
  .routine > :global([role="switch"]) { margin-top: 4px; }
  .name { justify-self: start; max-width: 100%; min-height: 22px; padding: 0; border: 0; background: none; color: var(--ink); font: inherit; font-size: var(--text-13); text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .name:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 2px; }
  .when { margin: 0; color: var(--muted); font: var(--text-12) var(--font-human); }
  .poll-error { color: var(--oxide); font: var(--text-12) var(--font-human); overflow-wrap: anywhere; }
</style>

<script>
  // The machines a Settings page reads and changes, named at the page's top:
  // All machines, or one machine that answers. Picking one machine moves the
  // composer's machine chip with it, since both select the same machine.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { ALL_MACHINES, kindIcon, machineName } from './runtimes.js'

  let { scope, all = true, note = '', disabled = false, onselect = () => {} } = $props()
  const value = $derived(all ? ALL_MACHINES : scope.selected.id)

  // The switch keeps naming the scope the page belongs to until it moves,
  // so a refused or pending switch never shows another one.
  function change(event) {
    const select = event.currentTarget
    const picked = select.value
    select.value = value
    if (picked === value) return
    if (picked === ALL_MACHINES) return onselect(ALL_MACHINES)
    const runtime = scope.choices.find((choice) => choice.id === picked)
    if (runtime) onselect(runtime)
  }
</script>

<div class="machine-scope" role="group" aria-label="Machine">
  <LucideIcon name={all ? 'globe' : kindIcon(scope.selected)} size={14} />
  <label for="settings-machine">Settings for</label>
  <select id="settings-machine" {value} {disabled} onchange={change}>
    <!-- Each option carries its selection, so a list that arrives before or
         after the selected machine changes still shows that machine. -->
    <option value={ALL_MACHINES} selected={all}>All machines</option>
    {#each scope.choices as runtime (runtime.id)}
      <option value={runtime.id} selected={!all && runtime.id === scope.selected.id}>{machineName(runtime)}</option>
    {/each}
  </select>
  {#if note}<span class="scope-note">{note}</span>{/if}
</div>

<style>
  .machine-scope { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; padding: 0 0 12px; color: var(--muted); font-size: var(--text-13); }
  label { flex: none; }
  select { min-width: 96px; max-width: 260px; padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; }
  select:disabled { color: var(--muted); }
  .scope-note { flex-basis: 100%; font-size: var(--text-12); }
</style>

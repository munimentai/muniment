<script>
  // The machine a Settings page reads and changes, named at the page's top,
  // with a switch to another machine that answers. The composer's machine
  // chip and this switch select the same machine.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { kindIcon } from './runtimes.js'

  let { scope, disabled = false, onselect = () => {} } = $props()

  function change(event) {
    const runtime = scope.choices.find((choice) => choice.id === event.currentTarget.value)
    if (runtime && runtime.id !== scope.selected.id) onselect(runtime)
  }
</script>

<div class="machine-scope" role="group" aria-label="Machine">
  <LucideIcon name={kindIcon(scope.selected)} size={14} />
  <label for="settings-machine">Settings for</label>
  <select id="settings-machine" value={scope.selected.id} {disabled} onchange={change}>
    {#each scope.choices as runtime (runtime.id)}
      <option value={runtime.id}>{runtime.name}</option>
    {/each}
  </select>
</div>

<style>
  .machine-scope { display: flex; align-items: center; gap: 8px; min-width: 0; padding: 0 0 12px; color: var(--muted); font-size: var(--text-13); }
  label { flex: none; }
  select { min-width: 0; max-width: 260px; padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; }
  select:disabled { color: var(--muted); }
</style>

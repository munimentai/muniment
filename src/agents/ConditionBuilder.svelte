<script>
  // One group of a tool check's condition: all or any of its checks and
  // nested groups. Each check reads one JSON pointer in the tool's result.
  import ConditionBuilder from './ConditionBuilder.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { OPERATORS, newCheck, newGroup, takesValue } from './condition.js'
  let { node = $bindable(), depth = 0, disabled = false, onremove = null } = $props()

  function remove(index) { node.items = node.items.filter((_, at) => at !== index) }
</script>

<div class="group" class:nested={depth > 0} role="group" aria-label={depth ? 'Nested condition group' : 'Condition'}>
  <div class="head">
    <select aria-label="Match" bind:value={node.join} {disabled}>
      <option value="all">All of these</option>
      <option value="any">Any of these</option>
    </select>
    {#if onremove}<button type="button" class="icon" aria-label="Remove group" {disabled} onclick={onremove}><LucideIcon name="x" size={14} variant="action" /></button>{/if}
  </div>
  {#each node.items as item, index (index)}
    {#if item.type === 'group'}
      <ConditionBuilder bind:node={node.items[index]} depth={depth + 1} {disabled} onremove={() => remove(index)} />
    {:else}
      <div class="check" role="group" aria-label={`Check ${index + 1}`}>
        <input class="mono" aria-label="Result path" placeholder="/items/0/status" bind:value={item.path} {disabled} spellcheck="false" autocomplete="off" />
        <select aria-label="Comparison" bind:value={item.op} {disabled}>{#each OPERATORS as [op, words] (op)}<option value={op}>{words}</option>{/each}</select>
        {#if takesValue(item.op)}<input class="mono" aria-label="Value" placeholder="value" bind:value={item.value} {disabled} spellcheck="false" autocomplete="off" />{/if}
        <button type="button" class="icon" aria-label={`Remove check ${index + 1}`} disabled={disabled || node.items.length === 1} onclick={() => remove(index)}><LucideIcon name="x" size={14} variant="action" /></button>
      </div>
    {/if}
  {/each}
  <div class="add">
    <button type="button" {disabled} onclick={() => { node.items = [...node.items, newCheck()] }}>Add check</button>
    {#if depth < 2}<button type="button" {disabled} onclick={() => { node.items = [...node.items, newGroup(node.join === 'all' ? 'any' : 'all')] }}>Add group</button>{/if}
  </div>
</div>

<style>
  .group { display: grid; gap: 6px; min-width: 0; }
  .nested { padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .head, .add { display: flex; align-items: center; gap: 6px; }
  .head select { flex: 1; }
  .check { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .check input { flex: 1 1 110px; }
  .check select { flex: 1 1 140px; }
  input, select { box-sizing: border-box; min-width: 0; min-height: 30px; padding: 5px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
  .mono { font-family: var(--font-mono); font-size: var(--text-12); }
  button { min-height: 28px; padding: 4px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; font-size: var(--text-12); cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { opacity: .5; cursor: default; }
  .icon { display: grid; place-items: center; min-width: 28px; padding: 0; border-color: transparent; color: var(--muted); }
</style>

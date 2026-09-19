<script>
  // The composer's model picker: every shown model by provider, the model in
  // use marked, a search over the ids, and Manage models at its foot.
  import { onMount, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { pickerGroups, sourceTag } from './provider-catalog.js'

  let { inventory, current, onchoose, onmanage, onclose } = $props()
  let query = $state('')
  let search = $state()
  const groups = $derived(pickerGroups(inventory, query))

  onMount(() => {
    void tick().then(() => search?.focus())
  })

  function keydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onclose()
  }
</script>

<div class="model-picker" role="dialog" aria-label="Model" onkeydown={keydown}>
  <div class="picker-search">
    <LucideIcon name="search" variant="action" size={14} />
    <input type="search" aria-label="Search models" placeholder="Search models" bind:this={search} bind:value={query}>
  </div>
  <div class="picker-list">
    {#if groups.length === 0}
      <p class="support">{inventory?.providers?.length ? 'No model matches.' : 'No provider is connected.'}</p>
    {/if}
    {#each groups as group (group.id)}
      <section class="picker-group" aria-label={group.name}>
        <h4>{group.name}{#if !group.classifier} <span class="tag">{sourceTag(group.source)}</span>{/if}</h4>
        <ul>
          {#each group.models as model (model.id)}
            {@const inUse = current?.provider === model.provider && current?.model === model.choice}
            <li>
              <button type="button" class="quiet picker-row" aria-pressed={inUse} onclick={() => onchoose(model.provider, model.choice)}>
                <span class="model-id" class:classifier={model.id === 'auto' && group.classifier}>{model.label ?? model.id}</span>
                {#if model.accounts > 1}<span class="balanced" aria-label={`Balanced across ${model.accounts} accounts`}><LucideIcon name="refresh-ccw-dot" variant="action" size={14} /><span class="tag">{model.accounts}</span></span>{/if}
                {#if inUse}<LucideIcon name="check" variant="action" size={14} />{/if}
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  </div>
  <button type="button" class="quiet picker-manage" onclick={onmanage}><LucideIcon name="settings" variant="action" size={14} />Models &amp; routing</button>
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .model-picker { position: absolute; z-index: 5; left: 0; bottom: calc(100% + 8px); width: min(380px, 100%); max-height: 60vh; display: flex; flex-direction: column; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); box-shadow: var(--shadow-overlay); }
  .picker-search { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border); color: var(--muted); }
  .picker-search input { flex: 1; min-width: 0; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; font-size: var(--text-13); }
  .picker-list { flex: 1; min-height: 0; padding: 6px; overflow-y: auto; }
  .picker-group h4 { display: flex; align-items: center; gap: 8px; margin: 6px 8px 2px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .tag { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .classifier { color: var(--ink); font-weight: 600; }
  .balanced { display: inline-flex; align-items: center; gap: 4px; margin-left: auto; color: var(--muted); }
  .picker-group ul { margin: 0; padding: 0; list-style: none; }
  .picker-row { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 5px 8px; text-align: left; font-size: var(--text-13); }
  .picker-row[aria-pressed="true"] { color: var(--ink); background: var(--faint); }
  .model-id { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); font-size: var(--text-12); }
  .support { margin: 8px; color: var(--muted); font-size: var(--text-13); }
  .picker-manage { display: flex; align-items: center; gap: 8px; width: 100%; padding: 9px 12px; border: 0; border-top: 1px solid var(--border); text-align: left; font-size: var(--text-13); }
</style>

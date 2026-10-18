<script>
  import PopupClose from './PopupClose.svelte'
  import { panelScroll } from './panel-scroll.js'
  // The composer's model picker: a column of provider marks, every connected
  // provider's current models under its name with its older ones folded under
  // Legacy models, the model in use marked, a search over every model, and
  // Model providers at its foot. A mark scrolls to its provider. Each model
  // row moves the model between the two lists.
  import { onMount, tick } from 'svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { classifierProvider } from './classifier-catalog.js'
  import { pickerGroups, providerName } from './provider-catalog.js'
  import { compactsOnSwitch, windowTokens } from './context-usage.js'

  let { inventory, current, contextUsage = null, onchoose, onvisibility = () => {}, onmanage, onrouting, onclose } = $props()
  let query = $state('')
  let search = $state()
  const searching = $derived(!!query.trim())
  const groups = $derived(pickerGroups(inventory, query, { all: true }).map((group) => {
    const providerId = group.classifier ? classifierProvider(group.classifier) : group.id.replace(/^router:/, '')
    return {
      ...group,
      providerId,
      label: group.classifier ? 'Model router' : (providerName(providerId) === providerId ? group.name : providerName(providerId)),
      shown: group.models.filter((model) => model.shown),
      legacy: group.models.filter((model) => !model.shown),
    }
  }))
  const inUse = (model) => current?.provider === model.provider && current?.model === model.choice
  // The picker opens scrolled to the provider of the model in use. The saved
  // model always stays in its provider's current list.
  let picked = $state(null)
  let legacyOpen = $state(new Set())
  let list = $state()
  const selected = $derived(groups.find((group) => group.id === picked) ?? groups.find((group) => group.models.some(inUse)) ?? groups[0] ?? null)
  let tip = $state(null)

  onMount(() => {
    void tick().then(() => {
      search?.focus()
      if (selected && selected !== groups[0]) reveal(selected)
    })
  })

  function reveal(group) {
    list?.querySelector(`[data-group="${CSS.escape(group.id)}"]`)?.scrollIntoView?.({ block: 'start' })
  }

  function toggleLegacy(group) {
    const next = new Set(legacyOpen)
    if (!next.delete(group.id)) next.add(group.id)
    legacyOpen = next
  }

  function keydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onclose()
  }

  async function pick(group) {
    picked = group.id
    query = ''
    await tick()
    reveal(group)
  }

  // Up and down arrows move between the provider marks.
  function railKeydown(event) {
    if (!['ArrowDown', 'ArrowUp'].includes(event.key)) return
    event.preventDefault()
    const index = groups.findIndex((group) => group.id === selected?.id)
    const next = groups[(index + (event.key === 'ArrowDown' ? 1 : groups.length - 1)) % groups.length]
    if (!next) return
    pick(next)
    const rail = event.currentTarget
    void tick().then(() => rail?.querySelector('[aria-current="true"]')?.focus())
  }

  // Hovering or focusing a mark names its provider beside the column.
  function showTip(event, group) {
    const mark = event.currentTarget
    tip = { label: group.label, top: mark.offsetTop - mark.parentElement.scrollTop + mark.offsetHeight / 2 }
  }
</script>

<svelte:window onkeydown={keydown} />

<div data-panel="model-picker" data-panel-variant="overlay" data-composer-panel class="model-picker" role="dialog" aria-label="Model">
  <div class="picker-search">
    <LucideIcon name="search" variant="action" size={14} />
    <input type="search" autocomplete="off" aria-label="Search models" placeholder="Search models" bind:this={search} bind:value={query}>
    <PopupClose label="Close model selector" onclick={onclose} />
  </div>
  <div class="picker-body">
    {#if groups.length}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <nav class="provider-rail" aria-label="Providers" onkeydown={railKeydown}>
        {#each groups as group (group.id)}
          {@const chosen = !searching && group.id === selected?.id}
          <button type="button" class="rail-mark" aria-label={group.label} aria-current={chosen ? 'true' : undefined} aria-controls="model-picker-list" tabindex={chosen || (searching && group === groups[0]) ? 0 : -1} onclick={() => pick(group)} onpointerenter={(event) => showTip(event, group)} onpointerleave={() => tip = null} onfocus={(event) => showTip(event, group)} onblur={() => tip = null}>
            {#if group.classifier}<LucideIcon name="route" variant="action" size={18} />{:else}<ProviderLogo color provider={group.providerId} size={18} />{/if}
          </button>
        {/each}
      </nav>
    {/if}
    {#if tip}<span class="rail-tip" aria-hidden="true" style:top="{tip.top}px">{tip.label}</span>{/if}
    <div class="picker-list" id="model-picker-list" role="region" aria-label={searching ? 'Search results' : 'Models'} bind:this={list} use:panelScroll>
      {#if groups.length === 0}
        <p class="support">{inventory?.providers?.length ? 'No model matches.' : 'No provider is connected.'}</p>
      {:else if searching}
        {#each groups as group (group.id)}
          <section class="picker-group" aria-label={group.label}>
            <h4>{group.label}</h4>
            <ul>{#each [...group.shown, ...group.legacy] as model (model.id)}{@render row(group, model)}{/each}</ul>
          </section>
        {/each}
      {:else}
        {#each groups as group (group.id)}
          {@const open = legacyOpen.has(group.id)}
          <section class="picker-group" aria-label={group.label} data-group={group.id}>
            {#if group.manage}<button type="button" class="quiet group-manage" aria-label="Manage model routing" onclick={onrouting}>Manage</button>{/if}
            <h4>{group.label}</h4>
            <ul>{#each group.shown as model (model.id)}{@render row(group, model)}{/each}</ul>
            {#if group.legacy.length}
              <button type="button" class="quiet legacy-toggle" aria-expanded={open} onclick={() => toggleLegacy(group)}>
                <span class="legacy-text"><span>Legacy models</span><span class="legacy-count">{group.legacy.length} {group.legacy.length === 1 ? 'model' : 'models'}</span></span>
                <LucideIcon name={open ? 'chevron-down' : 'chevron-right'} variant="action" size={14} />
              </button>
              {#if open}<ul aria-label="Legacy models">{#each group.legacy as model (model.id)}{@render row(group, model)}{/each}</ul>{/if}
            {/if}
          </section>
        {/each}
      {/if}
    </div>
  </div>
  <button type="button" class="quiet picker-manage" onclick={onmanage}><LucideIcon name="settings" variant="action" size={14} />Model providers</button>
</div>

{#snippet row(group, model)}
  {@const used = inUse(model)}
  {@const compacts = !used && compactsOnSwitch(contextUsage, model.contextWindow ?? windowTokens(model.context))}
  {@const auto = model.id === 'auto' && group.classifier}
  <li class="picker-item">
    <button type="button" class="quiet picker-row" data-provider={model.provider} data-model={model.choice} aria-pressed={used} onclick={() => onchoose(model.provider, model.choice)}>
      <span class="model-label">{#if auto}<ProviderLogo provider={group.providerId} size={16} />{/if}<span class="model-id" class:classifier={auto}>{model.label ?? model.id}</span>{#if compacts}<span class="will-compact"><LucideIcon name="combine" variant="action" size={12} />will compact</span>{/if}</span>
      {#if model.accounts > 1}<span class="balanced" aria-label={`Balanced across ${model.accounts} accounts`}><LucideIcon name="scale" variant="action" size={14} /><span class="tag">{model.accounts}</span></span>{/if}
      {#if used}<LucideIcon name="check" variant="action" size={14} />{/if}
    </button>
    {#if !group.classifier}
      <button type="button" class="quiet visibility" aria-label={`${model.shown ? 'Hide' : 'Show'} ${model.label ?? model.id}`} onclick={() => onvisibility(model, !model.shown)}><LucideIcon name={model.shown ? 'eye-off' : 'eye'} variant="action" size={14} /></button>
    {/if}
  </li>
{/snippet}

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .model-picker { width: min(420px, 100%); max-height: 60vh; display: flex; flex-direction: column; overflow: hidden; }
  .picker-search { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border); color: var(--muted); }
  .picker-search input { flex: 1; min-width: 0; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; font-size: var(--text-13); }
  .picker-body { position: relative; flex: 1; min-height: 0; display: flex; }
  /* The provider column: full-color marks, the picked one on the hover fill with an ink edge. */
  .provider-rail { flex: none; display: flex; flex-direction: column; gap: 4px; padding: 6px; border-right: 1px solid var(--border); overflow-y: auto; outline: 0; }
  .rail-mark { position: relative; display: grid; place-items: center; width: 34px; height: 34px; padding: 0; border-color: transparent; background: transparent; }
  .rail-mark[aria-current="true"] { background: var(--faint); }
  .rail-mark[aria-current="true"]::after { content: ''; position: absolute; top: 8px; bottom: 8px; right: -7px; width: 2px; border-radius: var(--radius-chip); background: var(--ink); }
  .rail-tip { position: absolute; z-index: 1; left: 52px; transform: translateY(-50%); padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
  .picker-list { flex: 1; min-width: 0; min-height: 0; padding: 6px; overflow-y: auto; }
  .picker-group h4 { display: flex; align-items: center; gap: 8px; margin: 6px 8px 2px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  /* Routing is off or has no decision model: the heading links to Decision models. */
  .picker-group { position: relative; scroll-margin-top: 0; }
  .picker-group + .picker-group { margin-top: 6px; }
  .group-manage { position: absolute; top: 0; right: 8px; padding: 2px 6px; border: 0; border-radius: var(--radius-control); background: none; color: var(--ink); font: var(--text-12) var(--font-human); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .group-manage:hover { background: var(--faint); }
  .tag { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .classifier { color: var(--ink); font-weight: 600; }
  .balanced { display: inline-flex; align-items: center; gap: 4px; margin-left: auto; color: var(--muted); }
  .will-compact { flex: none; display: inline-flex; align-items: center; gap: 4px; color: var(--ochre); font: var(--text-12) var(--font-human); }
  .picker-group ul { margin: 0; padding: 0; list-style: none; }
  .picker-item { position: relative; }
  .picker-row { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 5px 34px 5px 8px; text-align: left; font-size: var(--text-13); }
  .picker-row[aria-pressed="true"] { color: var(--ink); background: var(--faint); }
  /* The row's visibility control shows while the pointer or focus rests on the row. */
  .visibility { position: absolute; top: 50%; right: 4px; display: grid; place-items: center; width: 24px; height: 24px; padding: 0; transform: translateY(-50%); color: var(--muted); opacity: 0; }
  .picker-item:hover .visibility, .visibility:focus-visible { opacity: 1; }
  .visibility:hover:not(:disabled) { color: var(--ink); }
  .model-label { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .model-id { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); font-size: var(--text-12); }
  .legacy-toggle { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; margin-top: 2px; padding: 6px 8px; text-align: left; }
  .legacy-text { display: grid; gap: 2px; }
  .legacy-count { color: var(--muted); font-size: var(--text-12); }
  .support { margin: 8px; color: var(--muted); font-size: var(--text-13); }
  .picker-manage { position: relative; display: flex; align-items: center; gap: 8px; width: 100%; padding: 9px 12px; border: 0; text-align: left; font-size: var(--text-13); }
  /* The menu separator: a straight line inset to the list's 6px padding at three-quarter strength. */
  .picker-manage::before { content: ''; position: absolute; top: 0; left: 6px; right: 6px; height: 1px; background: var(--border); opacity: .75; }
</style>

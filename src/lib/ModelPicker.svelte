<script>
  import PopupClose from './PopupClose.svelte'
  import { panelScroll } from './panel-scroll.js'
  // The composer's model picker: a column of provider marks, every connected
  // provider's current models under its name with its older ones folded under
  // Legacy models, the model in use marked, a search over every model, and
  // Manage models at its foot. A mark filters the list to its provider. Each
  // model row moves the model between the two lists. `embedded` draws it open
  // inside another panel: no popup frame, close control or focus on mount.
  import { onMount, tick } from 'svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { classifierProvider } from './classifier-catalog.js'
  import { modelDisplayName, pickerGroups, providerName } from './provider-catalog.js'
  import { compactsOnSwitch, windowTokens } from './context-usage.js'

  // `source` names where the model in use comes from, such as `from project
  // Homelab`, and `actions` are the foot's choices for it, each run by `onaction`.
  // A row shows its visibility control only when `onvisibility` is given.
  let { inventory, current, contextUsage = null, source = '', actions = [], embedded = false, disabled = false, onaction = () => {}, onchoose, onvisibility = null, onmanage, onrouting, onclose = () => {} } = $props()
  const uid = $props.id()
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
  // The picker opens on the provider of the model in use. The saved model
  // always stays in its provider's current list.
  let picked = $state(null)
  let legacyOpen = $state(new Set())
  let list = $state()
  const selected = $derived(groups.find((group) => group.id === picked) ?? groups.find((group) => group.models.some(inUse)) ?? groups[0] ?? null)
  let tip = $state(null)

  onMount(() => {
    if (!embedded) void tick().then(() => search?.focus())
  })

  function toggleLegacy(group) {
    const next = new Set(legacyOpen)
    if (!next.delete(group.id)) next.add(group.id)
    legacyOpen = next
  }

  // Escape listens on the window's capture phase, so it reaches this menu
  // before the shell's document keys, and the page under it stays.
  function keydown(event) {
    if (embedded || event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onclose()
  }

  async function pick(group) {
    picked = group.id
    query = ''
    await tick()
    list?.scrollTo?.({ top: 0 })
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

  // Hovering or focusing a mark names its provider next to its icon and
  // never over the list: left of the picker, where free page lies there, else
  // just above the icon, over the rail, ending where the words of the list's
  // rows start. A name too long for the room there covers the sidebar's edge,
  // never a model name. An embedded picker has its card's rail at its left, so
  // its names show above the icon. The tip sits on the page, since the picker
  // clips what overflows it.
  const TIP_GAP = 4
  // The list's headings and rows inset their words 8px.
  const ROW_INSET = 8
  let picker = $state()
  function showTip(event, group) {
    tip = { label: group.label, mark: event.currentTarget }
  }
  function placeTip(node, { mark }) {
    const box = picker.getBoundingClientRect()
    const markBox = mark.getBoundingClientRect()
    // A logo with a light and a dark variant keeps the other theme's copy in
    // the mark, hidden with no box, so the tip measures the one that shows.
    const icon = ([...mark.querySelectorAll('svg, img')].map((node) => node.getBoundingClientRect()).find((rect) => rect.width && rect.height) ?? markBox)
    const sidebar = document.getElementById('sidebar')?.getBoundingClientRect()
    const page = sidebar?.width && sidebar.right <= box.left + 1 && sidebar.top < box.bottom && box.top < sidebar.bottom ? sidebar.right : 0
    const { width, height } = node.getBoundingClientRect()
    let left = box.left - TIP_GAP - width
    let top = icon.top + icon.height / 2 - height / 2
    if (embedded || left < page) {
      const rows = list ? list.getBoundingClientRect().left + parseFloat(getComputedStyle(list).paddingLeft) + ROW_INSET : markBox.right
      left = Math.max(0, rows - width)
      top = markBox.top - 2 - height
      if (top < 0) top = markBox.bottom + 2
    }
    Object.assign(node.style, { left: `${left}px`, top: `${top}px` })
  }
  function onPage(node, value) {
    document.body.append(node)
    placeTip(node, value)
    return { update(next) { placeTip(node, next) }, destroy() { node.remove() } }
  }
</script>

<svelte:window onkeydowncapture={keydown} />

<div bind:this={picker} data-panel="model-picker" data-panel-variant={embedded ? 'embedded' : 'overlay'} data-composer-panel={embedded ? undefined : ''} class="model-picker" class:embedded role={embedded ? 'group' : 'dialog'} aria-label="Model">
  <div class="picker-search">
    <LucideIcon name="search" variant="action" size={14} />
    <input type="search" data-focus-frame spellcheck="false" autocorrect="off" autocapitalize="off" autocomplete="off" aria-label="Search models" placeholder="Search models" bind:this={search} bind:value={query}>
    {#if !embedded}<PopupClose label="Close model selector" onclick={onclose} />{/if}
  </div>
  <div class="picker-body">
    {#if groups.length}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <nav class="provider-rail" aria-label="Providers" onkeydown={railKeydown}>
        {#each groups as group (group.id)}
          {@const chosen = !searching && group.id === selected?.id}
          <button type="button" class="rail-mark" aria-label={group.label} aria-current={chosen ? 'true' : undefined} aria-controls={`${uid}-list`} tabindex={chosen || (searching && group === groups[0]) ? 0 : -1} onclick={() => pick(group)} onpointerenter={(event) => showTip(event, group)} onpointerleave={() => tip = null} onfocus={(event) => showTip(event, group)} onblur={() => tip = null}>
            {#if group.classifier}<LucideIcon name="route" variant="action" size={18} />{:else}<ProviderLogo color provider={group.providerId} name={group.label} size={18} />{/if}
          </button>
        {/each}
      </nav>
    {/if}
    {#if tip}<span class="rail-tip" aria-hidden="true" use:onPage={tip}>{tip.label}</span>{/if}
    <div class="picker-list" id={`${uid}-list`} role="region" aria-label={searching ? 'Search results' : 'Models'} bind:this={list} use:panelScroll>
      {#if groups.length === 0}
        <p class="support">{inventory?.providers?.length ? 'No model matches.' : 'No provider is connected.'}</p>
      {:else if searching}
        {#each groups as group (group.id)}
          <section class="picker-group" aria-label={group.label}>
            <h4>{@render mark(group)}{group.label}</h4>
            {@render notAnswering(group)}
            <ul>{#each [...group.shown, ...group.legacy] as model (model.id)}{@render row(group, model)}{/each}</ul>
          </section>
        {/each}
      {:else if selected}
        {@const group = selected}
        {@const open = legacyOpen.has(group.id)}
        <section class="picker-group" aria-label={group.label} data-group={group.id}>
          {#if group.manage}<button type="button" class="quiet group-manage" aria-label="Manage model routing" onclick={onrouting}>Manage</button>{/if}
          <h4>{@render mark(group)}{group.label}</h4>
          {@render notAnswering(group)}
          <ul>{#each group.shown as model (model.id)}{@render row(group, model)}{/each}</ul>
          <!-- With no ready decision model the router has no row of its own: its pooled models list under their providers. -->
          {#if group.classifier && !group.models.length}<p class="support">{group.decisionModel ? 'Routing is off. Turn on Route each message in Settings › Models › Roles.' : 'Connect a decision model to route each message.'}</p>{/if}
          {#if group.legacy.length}
            <button type="button" class="quiet legacy-toggle" aria-expanded={open} onclick={() => toggleLegacy(group)}>
              <span class="legacy-text"><span>Legacy models</span><span class="legacy-count">{group.legacy.length} {group.legacy.length === 1 ? 'model' : 'models'}</span></span>
              <LucideIcon name={open ? 'chevron-down' : 'chevron-right'} variant="action" size={14} />
            </button>
            {#if open}{#if group.legacy.some((model) => model.served === false)}<p class="support">Show a legacy model to use it.</p>{/if}<ul aria-label="Legacy models">{#each group.legacy as model (model.id)}{@render row(group, model)}{/each}</ul>{/if}
          {/if}
        </section>
      {/if}
    </div>
  </div>
  {#if source && current}
    {@const label = groups.flatMap((group) => group.models).find(inUse)?.label ?? modelDisplayName(inventory, current.provider, current.model)}
    <div class="picker-source">
      <span class="source-line">{label} · {source}</span>
      {#each actions as action (action.id)}<button type="button" class="quiet source-action" {disabled} onclick={() => onaction(action.id)}>{action.label}</button>{/each}
    </div>
  {/if}
  <button type="button" class="quiet picker-manage" onclick={onmanage}><LucideIcon name="settings" variant="action" size={14} />Manage models</button>
</div>

<!-- An endpoint that did not answer the last refresh still lists its saved models. -->
{#snippet notAnswering(group)}{#if group.unreachable}<p class="support" data-testid="endpoint-not-answering">Not answering. Its saved models stay.</p>{/if}{/snippet}

{#snippet mark(group)}{#if group.classifier}<LucideIcon name="route" variant="action" size={14} />{:else}<ProviderLogo color provider={group.providerId} name={group.label} size={14} />{/if}{/snippet}

{#snippet row(group, model)}
  {@const used = inUse(model)}
  {@const compacts = !used && compactsOnSwitch(contextUsage, model.contextWindow ?? windowTokens(model.context))}
  {@const auto = model.id === 'auto' && group.classifier}
  {@const choosable = used || (!model.unavailable && model.served !== false)}
  <li class="picker-item">
    <button type="button" class="quiet picker-row" data-provider={model.provider} data-model={model.choice} aria-pressed={used} disabled={disabled || !choosable} aria-describedby={model.unavailable ? `${uid}-unavailable-${group.id}-${model.id}` : undefined} onclick={() => { if (choosable) onchoose(model.provider, model.choice) }}>
      <span class="model-label">{#if auto}<ProviderLogo provider={group.providerId} size={16} />{/if}<span class="model-id" class:classifier={auto}>{model.label ?? model.id}</span>{#if compacts}<span class="will-compact"><LucideIcon name="combine" variant="action" size={12} />will compact</span>{/if}{#if model.unavailable}<span class="unavailable" id={`${uid}-unavailable-${group.id}-${model.id}`}>Unavailable. {model.unavailable}</span>{/if}</span>
      {#if model.accounts > 1}<span class="balanced" aria-label={`Balanced across ${model.accounts} accounts`}><LucideIcon name="scale" variant="action" size={14} /><span class="tag">{model.accounts}</span></span>{/if}
      {#if used}<LucideIcon name="check" variant="action" size={14} />{/if}
    </button>
    {#if !group.classifier && onvisibility}
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
  .model-picker.embedded { width: 100%; height: 100%; max-height: none; background: transparent; }
  .picker-search { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border); color: var(--muted); }
  /* The row is the field's frame, so focus shows on its divider and the field draws no ring of its own. */
  .picker-search:focus-within { border-bottom-color: var(--muted); }
  .picker-search input { flex: 1; min-width: 0; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; font-size: var(--text-13); }
  .picker-body { position: relative; flex: 1; min-height: 0; display: flex; }
  /* The provider column: full-color marks, the picked one on the hover fill with an ink edge. */
  .provider-rail { flex: none; display: flex; flex-direction: column; gap: 4px; padding: 6px; border-right: 1px solid var(--border); overflow-y: auto; outline: 0; }
  .rail-mark { position: relative; display: grid; place-items: center; width: 34px; height: 34px; padding: 0; border-color: transparent; background: transparent; }
  .rail-mark[aria-current="true"] { background: var(--selected); }
  /* The edge spans the picked mark and meets the rail's divider, so it reads as the tab's edge and never as a text caret. */
  .rail-mark[aria-current="true"]::after { content: ''; position: absolute; top: 0; bottom: 0; right: -7px; width: 2px; background: var(--selected-edge); }
  .rail-tip { position: fixed; z-index: 30; padding: 2px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
  .picker-list { flex: 1; min-width: 0; min-height: 0; padding: 6px; overflow-y: auto; }
  .picker-group h4 { display: flex; align-items: center; gap: 8px; margin: 6px 8px 2px; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  /* Routing is off or has no decision model: the heading links to Roles in Models. */
  .picker-group { position: relative; scroll-margin-top: 0; }
  .picker-group + .picker-group { margin-top: 6px; }
  .group-manage { position: absolute; top: 0; right: 8px; padding: 2px 6px; border: 0; border-radius: var(--radius-control); background: none; color: var(--ink); font: var(--text-12) var(--font-human); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .group-manage:hover { background: var(--faint); }
  .tag { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .classifier { color: var(--ink); font-weight: 600; }
  .balanced { display: inline-flex; align-items: center; gap: 4px; margin-left: auto; color: var(--muted); }
  .unavailable { min-width: 0; color: var(--muted); font: var(--text-12) var(--font-human); white-space: normal; }
  .will-compact { flex: none; display: inline-flex; align-items: center; gap: 4px; color: var(--warning); font: var(--text-12) var(--font-human); }
  .picker-group ul { margin: 0; padding: 0; list-style: none; }
  .picker-item { position: relative; }
  .picker-row { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 5px 34px 5px 8px; text-align: left; font-size: var(--text-13); }
  .picker-row[aria-pressed="true"] { color: var(--on-selected); background: var(--selected); }
  /* The row's visibility control shows while the pointer or focus rests on the row. */
  .visibility { position: absolute; top: 50%; right: 4px; display: grid; place-items: center; width: 24px; height: 24px; padding: 0; transform: translateY(-50%); color: var(--muted); opacity: 0; }
  .picker-item:hover .visibility, .visibility:focus-visible { opacity: 1; }
  .visibility:hover:not(:disabled) { color: var(--ink); }
  .model-label { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .model-id { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-human); font-size: var(--text-13); }
  .legacy-toggle { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; margin-top: 2px; padding: 6px 8px; text-align: left; }
  .legacy-text { display: grid; gap: 2px; }
  .legacy-count { color: var(--muted); font-size: var(--text-12); }
  .support { margin: 8px; color: var(--muted); font-size: var(--text-13); }
  .picker-source { position: relative; display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; padding: 8px 12px; color: var(--muted); font-size: var(--text-12); }
  .picker-source::before { content: ''; position: absolute; top: 0; left: 6px; right: 6px; height: 1px; background: var(--border); opacity: .75; }
  .source-line { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .source-action { flex: none; padding: 2px 6px; border: 0; color: var(--ink); font-size: var(--text-12); text-decoration: underline; text-underline-offset: 2px; }
  .picker-manage { position: relative; display: flex; align-items: center; gap: 8px; width: 100%; padding: 9px 12px; border: 0; text-align: left; font-size: var(--text-13); }
  /* The menu separator: a straight line inset to the list's 6px padding at three-quarter strength. */
  .picker-manage::before { content: ''; position: absolute; top: 0; left: 6px; right: 6px; height: 1px; background: var(--border); opacity: .75; }
</style>

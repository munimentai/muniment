<script>
  // Settings → Search: the searches agents try in the user's order, the
  // engines built-in search asks, the providers the user adds keys for, then
  // the Browser tab's engine. The runtime keeps the record, and every change
  // answers the settings it left, which replace the page's copy. A key goes
  // from its input to the runtime and the page keeps only its last four
  // characters.
  import { onDestroy, onMount } from 'svelte'
  import BrowserSearch from './BrowserSearch.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import Toggle from './Toggle.svelte'
  import Button from './ui/Button.svelte'
  import DisclosureSummary from './ui/DisclosureSummary.svelte'
  import Group from './ui/Group.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Row from './ui/Row.svelte'
  import PocketFold from './PocketFold.svelte'
  import LucideIcon from './LucideIcon.svelte'

  let { tauri } = $props()

  // Providers whose mark the model settings already ship keep that file.
  const SHARED_LOGOS = { mistral: 'mistral', ollama: 'ollama', openai: 'openai', xai: 'xai', kimi: 'kimi-coding', zai: 'zai', gemini: 'google' }
  const logo = id => SHARED_LOGOS[id] ?? `search-${id}`

  const ENGINES = [
    { id: 'brave', name: 'Brave', note: 'Web results.' },
    { id: 'duckduckgo', name: 'DuckDuckGo', note: 'Web results.' },
    { id: 'wikipedia', name: 'Wikipedia', note: 'Encyclopedia articles.' },
    { id: 'yahoo', name: 'Yahoo', note: 'Web results.' },
    { id: 'bing', name: 'Bing', note: 'Web results.' },
  ]
  // `url` shows the server URL field for a provider the user can host.
  const COMMON = [
    { id: 'exa', name: 'Exa', note: 'Neural search with page highlights.' },
    { id: 'tavily', name: 'Tavily', note: 'Search with a short answer and page text.' },
    { id: 'firecrawl', name: 'Firecrawl', note: 'Search and page reading. Takes a server URL for a self-hosted Firecrawl.', url: true },
    { id: 'brave', name: 'Brave Search API', note: 'Brave’s paid index.' },
    { id: 'perplexity', name: 'Perplexity', note: 'Search with a written answer.' },
    { id: 'serper', name: 'Serper', note: 'Google results.' },
    { id: 'serpapi', name: 'SerpApi', note: 'Google and other engines.' },
    { id: 'kagi', name: 'Kagi', note: 'Search and page summaries.' },
    { id: 'jina', name: 'Jina', note: 'Search and page reading.' },
    { id: 'parallel', name: 'Parallel', note: 'Search and page reading for agents.' },
    { id: 'you', name: 'You.com', note: 'Web and news results.' },
  ]
  const MORE = [
    ['anysearch', 'AnySearch'], ['baizhi', '百智 Baizhi'], ['bocha', 'Bocha'], ['brightdata', 'Bright Data'], ['ceramic', 'Ceramic'], ['degoog', 'degoog', true],
    ['gemini', 'Gemini'], ['keenable', 'Keenable'], ['mistral', 'Mistral'], ['ollama', 'Ollama'], ['querit', 'Querit'], ['search1api', 'Search1API'], ['searchinfinity', 'SearchInfinity'],
    ['serpbase', 'SerpBase'], ['serpdive', 'SerpDive'], ['serply', 'Serply'], ['tinyfish', 'TinyFish'], ['valyu', 'Valyu'], ['xcrawl', 'xCrawl'], ['zai', 'Zhipu'],
  ].map(([id, name, url = false]) => ({ id, name, url }))
  // OpenAI and xAI search with the account connected in Model providers.
  const ACCOUNT_SEARCH = { openai: 'OpenAI', xai: 'xAI' }
  const FIELDS = { brightdataSerpZone: 'SERP zone', brightdataUnlockerZone: 'Unlocker zone' }
  const NAMES = Object.fromEntries([...COMMON, ...MORE].map(provider => [provider.id, provider.name]).concat(Object.entries(ACCOUNT_SEARCH)))
  const providerName = id => NAMES[id] ?? id

  let settings = $state(null)
  let order = $state([])
  let engines = $state({})
  let error = $state('')
  let adding = $state(null)
  let draftKey = $state('')
  let draftUrl = $state('')
  let draftExtra = $state({})
  let saving = $state(false)

  const keys = $derived(Object.fromEntries((settings?.keys ?? []).map(key => [key.provider, key])))
  const specs = $derived(Object.fromEntries((settings?.providers ?? []).map(provider => [provider.id, provider])))
  const open = id => specs[id] && !keys[id] && !ACCOUNT_SEARCH[id]
  const available = $derived(COMMON.filter(provider => open(provider.id)))
  const more = $derived([
    ...MORE.filter(provider => open(provider.id)),
    ...Object.keys(specs).filter(id => open(id) && !NAMES[id]).map(id => ({ id, name: id, url: false })),
  ])
  const searching = $derived(order.some(item => item.enabled))

  function describe(item) {
    if (item.id === 'muniment') return { name: 'Muniment search', note: 'Built in and free. Needs no key.' }
    if (item.id === 'exa' && !item.has_key) return { name: 'Exa free', note: 'Keyless, with shared limits.' }
    return { name: providerName(item.id), note: 'Your key.' }
  }
  const meta = item => item.has_key ? `••••${keys[item.id]?.last4 ?? ''}` : ''

  function apply(next) {
    settings = next
    order = next.order.map(({ id, enabled, has_key }) => ({ id, enabled, has_key }))
    engines = Object.fromEntries(next.engines.map(({ id, enabled }) => [id, enabled]))
  }

  // Each change runs in turn and only the latest answer replaces the page's
  // copy, so a slow answer never undoes a newer change. A refused change
  // reloads the record so the page shows what the runtime kept.
  let sent = 0
  async function send(command, args) {
    const mine = ++sent
    try {
      const next = await tauri.invoke(command, args)
      if (mine === sent) { apply(next); error = '' }
      return true
    } catch (failure) {
      error = String(failure?.message ?? failure)
      if (mine === sent) await reload()
      return false
    }
  }
  async function reload() {
    try { apply(await tauri.invoke('web_search_settings')) }
    catch (failure) { error = String(failure?.message ?? failure) }
  }
  onMount(reload)

  const saveOrder = () => send('web_search_save_order', { order: order.map(({ id, enabled }) => ({ id, enabled })) })
  const saveEngines = () => send('web_search_save_engines', { engines: ENGINES.map(engine => engine.id).filter(id => engines[id]) })

  // Arrow keys save once the moves stop, so a run of presses saves once.
  let pending = null
  function saveSoon() {
    clearTimeout(pending)
    pending = setTimeout(() => { pending = null; saveOrder() }, 400)
  }
  function saveNow() {
    if (pending === null) return
    clearTimeout(pending)
    pending = null
    saveOrder()
  }
  onDestroy(saveNow)

  function move(index, step) {
    const target = index + step
    if (target < 0 || target >= order.length) return false
    const next = [...order]
    const [item] = next.splice(index, 1)
    next.splice(target, 0, item)
    order = next
    return true
  }

  // The grip drags a search to a new place. The row follows the pointer's
  // midpoint crossings, and the arrow keys move it one place on the grip.
  let rows = $state([])
  let dragging = $state(null)
  let before = ''
  const sequence = () => order.map(item => item.id).join(' ')
  function grab(event, index) {
    if (event.button !== 0) return
    event.preventDefault()
    dragging = index
    before = sequence()
    document.documentElement.classList.add('search-order-dragging')
    // The row moves in the list while it drags, so the window follows the pointer.
    window.addEventListener('pointermove', drag)
    window.addEventListener('pointerup', drop, { once: true })
    window.addEventListener('pointercancel', drop, { once: true })
  }
  function drag(event) {
    if (dragging === null) return
    const over = rows.findIndex(row => { const box = row?.getBoundingClientRect(); return box && event.clientY >= box.top && event.clientY < box.bottom })
    if (over === -1 || over === dragging) return
    const box = rows[over].getBoundingClientRect()
    const past = over > dragging ? event.clientY > box.top + box.height / 2 : event.clientY < box.top + box.height / 2
    if (!past) return
    move(dragging, over - dragging)
    dragging = over
  }
  function drop() {
    window.removeEventListener('pointermove', drag)
    window.removeEventListener('pointerup', drop)
    window.removeEventListener('pointercancel', drop)
    dragging = null
    document.documentElement.classList.remove('search-order-dragging')
    if (sequence() !== before) saveOrder()
  }
  function gripKey(event, index) {
    const step = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0
    if (!step) return
    event.preventDefault()
    if (!move(index, step)) return
    saveSoon()
    queueMicrotask(() => rows[index + step]?.querySelector('.grip')?.focus())
  }

  function toggleSearch(item, value) {
    item.enabled = value
    clearTimeout(pending)
    pending = null
    saveOrder()
  }
  function toggleEngine(id, value) {
    engines[id] = value
    saveEngines()
  }

  function startKey(id) {
    adding = id
    error = ''
    draftKey = ''
    draftUrl = keys[id]?.base_url ?? ''
    draftExtra = Object.fromEntries((specs[id]?.extra ?? []).map(name => [name, keys[id]?.extra?.[name] ?? '']))
  }
  function cancelKey() {
    adding = null
    draftKey = ''
    draftUrl = ''
    draftExtra = {}
  }
  async function saveKey(provider) {
    if (!draftKey.trim() || saving) return
    saving = true
    const extra = Object.keys(draftExtra).length ? { ...draftExtra } : null
    const args = { provider: provider.id, apiKey: draftKey, baseUrl: provider.url && draftUrl.trim() ? draftUrl.trim() : null, extra }
    draftKey = ''
    const saved = await send('web_search_set_key', args)
    saving = false
    if (saved) cancelKey()
  }
  const removeKey = id => send('web_search_remove_key', { provider: id })
  const urlField = provider => provider.url && specs[provider.id]?.base_url
</script>

{#snippet mark(id, size = 20)}
  {#if id === 'muniment'}
    <span class="muniment-mark" style:width={`${size}px`} style:height={`${size}px`}><PocketFold {size} /></span>
  {:else}
    <ProviderLogo provider={logo(id)} {size} />
  {/if}
{/snippet}

{#snippet keyForm(provider)}
  <div class="key-form">
    {#if urlField(provider)}<input type="url" placeholder="Server URL (optional)" aria-label={`${provider.name} server URL`} autocomplete="off" bind:value={draftUrl}>{/if}
    {#each Object.keys(draftExtra) as field (field)}
      <input type="text" placeholder={FIELDS[field] ?? field} aria-label={`${provider.name} ${FIELDS[field] ?? field}`} autocomplete="off" bind:value={draftExtra[field]}>
    {/each}
    <input type="password" placeholder="API key" aria-label={`${provider.name} API key`} autocomplete="off" bind:value={draftKey} onkeydown={event => { if (event.key === 'Enter') saveKey(provider) }}>
    <Button variant="primary" disabled={saving || !draftKey.trim()} onclick={() => saveKey(provider)}>Save</Button>
    <Button onclick={cancelKey}>Cancel</Button>
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
{/snippet}

<div class="search-preferences">
  <PageHead intro="Choose how agents search the web, and the engine the Browser tab address bar uses." />

  <Group heading="Search order" description="Agents try each search that is on, top to bottom, and move to the next one when it fails or runs out of quota.">
    {#each order as item, index (item.id + (item.has_key ? '-key' : ''))}
      {@const shown = describe(item)}
      {@const provider = { id: item.id, name: shown.name, url: [...COMMON, ...MORE].some(entry => entry.id === item.id && entry.url) }}
      <div class="order-row" class:dragged={dragging === index} bind:this={rows[index]}>
      <Row title={shown.name} meta={meta(item)} description={shown.note} stacked={adding === item.id && item.has_key}>
        {#snippet lead()}<button type="button" class="grip" aria-label={`Reorder ${shown.name}, position ${index + 1} of ${order.length}`} aria-keyshortcuts="ArrowUp ArrowDown" onpointerdown={event => grab(event, index)} onkeydown={event => gripKey(event, index)}><LucideIcon name="grip-vertical" size={16} variant="action" /></button><span class="position">{index + 1}</span>{@render mark(item.id)}{/snippet}
        {#if adding === item.id && item.has_key}
          {@render keyForm(provider)}
        {:else}
          {#if item.has_key}
            <Button icon="square-pen" iconOnly aria-label={`Edit ${shown.name} key`} onclick={() => startKey(item.id)} />
            <Button icon="unplug" iconOnly aria-label={`Remove ${shown.name}`} onclick={() => removeKey(item.id)} />
          {/if}
          <Toggle checked={item.enabled} label={`Use ${shown.name}`} onchange={value => toggleSearch(item, value)} />
        {/if}
      </Row>
      </div>
    {/each}
    {#if settings && !searching}<p role="alert">Every search is off, so agents cannot search the web.</p>{/if}
    {#if settings}<p class="applies">Changes apply to the next agent session.</p>{/if}
  </Group>
  {#if error && !adding}<p role="alert">{error}</p>{/if}

  <Group heading="Muniment search engines" description="Muniment search asks these engines at once from this device and merges their results.">
    {#each ENGINES as engine (engine.id)}
      <Row title={engine.name} description={engine.note}>
        {#snippet lead()}{@render mark(engine.id)}{/snippet}
        <Toggle checked={engines[engine.id] ?? false} disabled={!settings} label={`Search ${engine.name}`} onchange={value => toggleEngine(engine.id, value)} />
      </Row>
    {/each}
  </Group>

  <Group heading="Add a search provider" description="Use a provider you already pay for. Muniment keeps the key on this device.">
    {#each available as provider (provider.id)}
      <Row title={provider.name} description={provider.note} stacked={adding === provider.id}>
        {#snippet lead()}{@render mark(provider.id)}{/snippet}
        {#if adding === provider.id}
          {@render keyForm(provider)}
        {:else}
          <Button icon="key-round" onclick={() => startKey(provider.id)}>Add key</Button>
        {/if}
      </Row>
    {/each}
    {#if more.length}
      <details class="more">
        <DisclosureSummary>More providers ({more.length})</DisclosureSummary>
        <ul>{#each more as provider (provider.id)}<li class:adding={adding === provider.id}><span class="more-name">{@render mark(provider.id, 16)}{provider.name}</span>{#if adding === provider.id}{@render keyForm(provider)}{:else}<Button icon="key-round" onclick={() => startKey(provider.id)}>Add key</Button>{/if}</li>{/each}</ul>
      </details>
    {/if}
    <p>OpenAI and xAI search with the account you connected in Model providers, so they need no key here.</p>
  </Group>

  <BrowserSearch />
</div>

<style>
  .search-preferences { display: grid; gap: var(--page-gap); }
  .grip { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 32px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: grab; touch-action: none; }
  .grip:hover, .grip:focus-visible { background: var(--faint); color: var(--ink); }
  .order-row.dragged { position: relative; z-index: 1; margin: 0 -8px; padding: 0 8px; background: var(--faint); border-radius: var(--radius-control); }
  :global(html.search-order-dragging), :global(html.search-order-dragging *) { cursor: grabbing !important; user-select: none; }
  .position { width: 16px; color: var(--muted); font: var(--text-12) var(--font-mono); text-align: right; }
  .search-preferences :global(.row-lead) { gap: 10px; }
  .muniment-mark { flex: none; display: inline-flex; align-items: center; justify-content: center; }
  .key-form { display: flex; flex-wrap: wrap; gap: 8px; width: 100%; }
  .key-form input { flex: 1 1 200px; min-width: 0; }
  .key-form p { flex: 1 1 100%; margin: 0; }
  .applies { color: var(--muted); font-size: var(--text-13); }
  .more { padding: 10px 0; }
  .more ul { display: grid; gap: 4px; margin: 8px 0 0; padding: 0 0 0 22px; list-style: none; }
  .more li { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; font-size: var(--text-13); }
  .more li.adding { padding-bottom: 8px; }
  .more-name { display: inline-flex; align-items: center; gap: 10px; }
</style>

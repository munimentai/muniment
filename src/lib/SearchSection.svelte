<script>
  // Settings → Search: the searches agents try in the user's order, the
  // engines built-in search asks, the providers the user adds keys for, then
  // the Browser tab's engine.
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

  // Providers whose mark the model settings already ship keep that file.
  const SHARED_LOGOS = { mistral: 'mistral', ollama: 'ollama', openai: 'openai', xai: 'xai', kimi: 'kimi-coding', zai: 'zai' }
  const logo = id => SHARED_LOGOS[id] ?? `search-${id}`

  const ENGINES = [
    { id: 'brave', name: 'Brave', note: 'Web results.' },
    { id: 'duckduckgo', name: 'DuckDuckGo', note: 'Web results.' },
    { id: 'wikipedia', name: 'Wikipedia', note: 'Encyclopedia articles.' },
    { id: 'yahoo', name: 'Yahoo', note: 'Web results.' },
    { id: 'bing', name: 'Bing', note: 'Web results.' },
  ]
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
    ['baizhi', '百智 Baizhi'], ['bocha', 'Bocha'], ['brightdata', 'Bright Data'], ['ceramic', 'Ceramic'], ['crawl4ai', 'Crawl4AI'],
    ['mistral', 'Mistral'], ['ollama', 'Ollama'], ['querit', 'Querit'], ['search1api', 'Search1API'], ['searchinfinity', 'SearchInfinity'],
    ['serpbase', 'SerpBase'], ['serpdive', 'SerpDive'], ['serply', 'Serply'], ['tinyfish', 'TinyFish'], ['valyu', 'Valyu'], ['xcrawl', 'xCrawl'], ['zai', 'Z.ai'],
  ]

  let engines = $state(Object.fromEntries(ENGINES.map(engine => [engine.id, true])))
  let order = $state([
    { id: 'muniment', name: 'Muniment search', note: 'Built in and free. Needs no key.', enabled: true },
    { id: 'tavily', name: 'Tavily', note: 'Your key.', key: '••••3f2a', enabled: true },
    { id: 'exa', name: 'Exa free', note: 'Keyless, with shared limits.', enabled: true },
  ])
  let adding = $state(null)
  let draftKey = $state('')
  const available = $derived(COMMON.filter(provider => !order.some(item => item.key && item.id === provider.id)))
  const searching = $derived(order.some(item => item.enabled))

  function move(index, step) {
    const target = index + step
    if (target < 0 || target >= order.length) return
    const next = [...order]
    const [item] = next.splice(index, 1)
    next.splice(target, 0, item)
    order = next
  }

  // The grip drags a search to a new place. The row follows the pointer's
  // midpoint crossings, and the arrow keys move it one place on the grip.
  let rows = $state([])
  let dragging = $state(null)
  function grab(event, index) {
    if (event.button !== 0) return
    event.preventDefault()
    dragging = index
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
  }
  function gripKey(event, index) {
    const step = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0
    if (!step) return
    event.preventDefault()
    move(index, step)
    queueMicrotask(() => rows[index + step]?.querySelector('.grip')?.focus())
  }
  function save(provider) {
    if (!draftKey.trim()) return
    order = [...order, { id: provider.id, name: provider.name, note: 'Your key.', key: `••••${draftKey.trim().slice(-4)}`, enabled: true }]
    adding = null
    draftKey = ''
  }
</script>

{#snippet mark(id, size = 20)}
  {#if id === 'muniment'}
    <span class="muniment-mark" style:width={`${size}px`} style:height={`${size}px`}><PocketFold {size} /></span>
  {:else}
    <ProviderLogo provider={logo(id)} {size} />
  {/if}
{/snippet}

<div class="search-preferences">
  <PageHead intro="Choose how agents search the web, and the engine the Browser tab address bar uses." />

  <Group heading="Search order" description="Agents try each search that is on, top to bottom, and move to the next one when it fails or runs out of quota.">
    {#each order as item, index (item.id + (item.key ? '-key' : ''))}
      <div class="order-row" class:dragged={dragging === index} bind:this={rows[index]}>
      <Row title={item.name} meta={item.key ?? ''} description={item.note}>
        {#snippet lead()}<button type="button" class="grip" aria-label={`Reorder ${item.name}, position ${index + 1} of ${order.length}`} aria-keyshortcuts="ArrowUp ArrowDown" onpointerdown={event => grab(event, index)} onkeydown={event => gripKey(event, index)}><LucideIcon name="grip-vertical" size={16} variant="action" /></button><span class="position">{index + 1}</span>{@render mark(item.id)}{/snippet}
        {#if item.key}
          <Button icon="square-pen" iconOnly aria-label={`Edit ${item.name} key`} />
          <Button icon="unplug" iconOnly aria-label={`Remove ${item.name}`} onclick={() => { order = order.filter(entry => entry !== item) }} />
        {/if}
        <Toggle checked={item.enabled} label={`Use ${item.name}`} onchange={value => { item.enabled = value }} />
      </Row>
      </div>
    {/each}
    {#if !searching}<p role="alert">Every search is off, so agents cannot search the web.</p>{/if}
  </Group>

  <Group heading="Muniment search engines" description="Muniment search asks these engines at once from this device and merges their results.">
    {#each ENGINES as engine (engine.id)}
      <Row title={engine.name} description={engine.note}>
        {#snippet lead()}{@render mark(engine.id)}{/snippet}
        <Toggle checked={engines[engine.id]} label={`Search ${engine.name}`} onchange={value => { engines[engine.id] = value }} />
      </Row>
    {/each}
  </Group>

  <Group heading="Add a search provider" description="Use a provider you already pay for. Muniment keeps the key on this device.">
    {#each available as provider (provider.id)}
      <Row title={provider.name} description={provider.note} stacked={adding === provider.id}>
        {#snippet lead()}{@render mark(provider.id)}{/snippet}
        {#if adding === provider.id}
          <div class="key-form">
            {#if provider.url}<input type="url" placeholder="Server URL (optional)" aria-label={`${provider.name} server URL`} autocomplete="off">{/if}
            <input type="password" placeholder="API key" aria-label={`${provider.name} API key`} autocomplete="off" bind:value={draftKey}>
            <Button variant="outline" onclick={() => save(provider)}>Save</Button>
            <Button onclick={() => { adding = null; draftKey = '' }}>Cancel</Button>
          </div>
        {:else}
          <Button icon="key-round" onclick={() => { adding = provider.id; draftKey = '' }}>Add key</Button>
        {/if}
      </Row>
    {/each}
    <details class="more">
      <DisclosureSummary>More providers ({MORE.length})</DisclosureSummary>
      <ul>{#each MORE as [id, name] (id)}<li><span class="more-name">{@render mark(id, 16)}{name}</span><Button icon="key-round">Add key</Button></li>{/each}</ul>
    </details>
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
  .more { padding: 10px 0; }
  .more ul { display: grid; gap: 4px; margin: 8px 0 0; padding: 0 0 0 22px; list-style: none; }
  .more li { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; font-size: var(--text-13); }
  .more-name { display: inline-flex; align-items: center; gap: 10px; }
</style>

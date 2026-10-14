<script>
  // Settings → Search: the built-in search agents use, the providers the user
  // adds keys for in the order agents try them, then the Browser tab's engine.
  import BrowserSearch from './BrowserSearch.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import Toggle from './Toggle.svelte'
  import Button from './ui/Button.svelte'
  import DisclosureSummary from './ui/DisclosureSummary.svelte'
  import Group from './ui/Group.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Row from './ui/Row.svelte'

  const ENGINES = ['Brave', 'DuckDuckGo', 'Wikipedia', 'Yahoo', 'Bing']
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
  const MORE = ['Bocha', 'Bright Data', 'Ceramic', 'Crawl4AI', 'Mistral', 'Ollama', 'Querit', 'Search1API', 'SearchInfinity', 'SerpBase', 'SerpDive', 'Serply', 'TinyFish', 'Valyu', 'xCrawl', 'Z.ai', '百智 Baizhi']

  let enabled = $state(Object.fromEntries(ENGINES.map(name => [name, true])))
  let added = $state([{ id: 'tavily', name: 'Tavily', key: '••••3f2a' }])
  let adding = $state(null)
  let draftKey = $state('')
  const available = $derived(COMMON.filter(provider => !added.some(item => item.id === provider.id)))

  function move(index, step) {
    const next = [...added]
    const [item] = next.splice(index, 1)
    next.splice(index + step, 0, item)
    added = next
  }
  function save(provider) {
    if (!draftKey.trim()) return
    added = [...added, { id: provider.id, name: provider.name, key: `••••${draftKey.trim().slice(-4)}` }]
    adding = null
    draftKey = ''
  }
</script>

<div class="search-preferences">
  <PageHead intro="Choose how agents search the web, and the engine the Browser tab address bar uses." />

  <Group heading="Muniment search" description="Built in and free. Searches several engines at once from this device, with no key and no account.">
    <Row title="Muniment search" meta="default" description="Agents search here first. It needs no setup.">
      {#snippet lead()}<span class="mark"><LucideIcon name="search" size={16} variant="action" /></span>{/snippet}
      <span class="ready">Ready</span>
    </Row>
    {#each ENGINES as name}
      <Row title={name} description={name === 'Wikipedia' ? 'Encyclopedia articles.' : 'Web results.'}>
        <Toggle checked={enabled[name]} label={`Search ${name}`} onchange={value => { enabled[name] = value }} />
      </Row>
    {/each}
  </Group>

  <Group heading="Search order" description="Agents try each search in this order and move to the next one when it fails or runs out of quota.">
    <Row title="1. Muniment search" description="Always first.">
      <span class="fixed">Built in</span>
    </Row>
    {#each added as provider, index (provider.id)}
      <Row title={`${index + 2}. ${provider.name}`} meta={provider.key} description="Your key.">
        <Button icon="arrow-up" iconOnly aria-label={`Move ${provider.name} up`} disabled={index === 0} onclick={() => move(index, -1)} />
        <Button icon="arrow-down" iconOnly aria-label={`Move ${provider.name} down`} disabled={index === added.length - 1} onclick={() => move(index, 1)} />
        <Button icon="square-pen" iconOnly aria-label={`Edit ${provider.name} key`} />
        <Button icon="unplug" iconOnly aria-label={`Remove ${provider.name}`} onclick={() => { added = added.filter(item => item.id !== provider.id) }} />
      </Row>
    {/each}
    <Row title={`${added.length + 2}. Exa free`} description="Keyless last resort with shared limits.">
      <span class="fixed">Built in</span>
    </Row>
  </Group>

  <Group heading="Add a search provider" description="Use a provider you already pay for. Muniment keeps the key on this device.">
    {#each available as provider (provider.id)}
      <Row title={provider.name} description={provider.note} stacked={adding === provider.id}>
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
      <ul>{#each MORE as name}<li><span>{name}</span><Button icon="key-round">Add key</Button></li>{/each}</ul>
    </details>
    <p>OpenAI, xAI and Kimi search with your signed-in model account, so they need no key here.</p>
  </Group>

  <BrowserSearch />
</div>

<style>
  .search-preferences { display: grid; gap: var(--page-gap); }
  .mark { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; border-radius: var(--radius-control); background: var(--signal-soft); color: var(--signal); }
  .ready { color: var(--signal); font: var(--text-12) var(--font-mono); }
  .fixed { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .key-form { display: flex; flex-wrap: wrap; gap: 8px; width: 100%; }
  .key-form input { flex: 1 1 200px; min-width: 0; }
  .more { padding: 10px 0; }
  .more ul { display: grid; gap: 4px; margin: 8px 0 0; padding: 0 0 0 22px; list-style: none; }
  .more li { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; font-size: var(--text-13); }
</style>

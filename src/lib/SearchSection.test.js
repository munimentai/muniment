import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import SearchSection from './SearchSection.svelte'

afterEach(() => { cleanup(); localStorage.clear(); vi.useRealTimers() })

const ENGINES = ['brave', 'bing', 'duckduckgo', 'yahoo', 'wikipedia']
const PROVIDERS = ['exa', 'tavily', 'firecrawl', 'brave', 'kagi', 'brightdata', 'openai']
  .map(id => ({ id, base_url: ['exa', 'tavily', 'firecrawl', 'brave'].includes(id), extra: id === 'brightdata' ? ['brightdataSerpZone', 'brightdataUnlockerZone'] : [] }))

function settings({ order = [['muniment', true], ['tavily', true], ['exa', true]], engines = ENGINES, keys = [{ provider: 'tavily', has_key: true, last4: '3f2a' }] } = {}) {
  const keyed = new Set(keys.map(key => key.provider))
  return {
    order: order.map(([id, enabled]) => ({ id, enabled, has_key: keyed.has(id) })),
    engines: ENGINES.map(id => ({ id, enabled: engines.includes(id) })),
    keys,
    providers: PROVIDERS,
    running: true,
  }
}

function setup(answers = {}) {
  const initial = settings()
  const invoke = vi.fn(async (command, args) => {
    if (answers[command]) return answers[command](args)
    return initial
  })
  render(SearchSection, { tauri: { invoke } })
  return invoke
}

const orderNames = () => screen.getAllByRole('button', { name: /^Reorder / }).map(grip => grip.getAttribute('aria-label').replace(/^Reorder (.*), position.*$/, '$1'))

it('renders the search order and the key meta from the runtime settings', async () => {
  const invoke = setup()
  await waitFor(() => expect(orderNames()).toEqual(['Muniment search', 'Tavily', 'Exa free']))
  expect(invoke).toHaveBeenCalledWith('web_search_settings')
  expect(screen.getByText('••••3f2a')).toBeInTheDocument()
  expect(screen.getByText('Changes apply to the next agent session.')).toBeInTheDocument()
  // Tavily holds a key, so only the providers without one offer Add key.
  const add = screen.getByRole('heading', { name: 'Add a search provider' }).closest('section')
  expect(within(add).queryByText('Tavily')).not.toBeInTheDocument()
  expect(within(add).getByText('Exa', { selector: 'strong' })).toBeInTheDocument()
  expect(within(add).getByText('More providers (1)')).toBeInTheDocument()
})

it('saves the new order after the arrow keys move a search', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true })
  const invoke = setup({ web_search_save_order: ({ order }) => settings({ order: order.map(({ id, enabled }) => [id, enabled]) }) })
  await waitFor(() => expect(orderNames()).toHaveLength(3))
  await fireEvent.keyDown(screen.getByRole('button', { name: /^Reorder Exa free/ }), { key: 'ArrowUp' })
  expect(orderNames()).toEqual(['Muniment search', 'Exa free', 'Tavily'])
  await vi.advanceTimersByTimeAsync(500)
  expect(invoke).toHaveBeenCalledWith('web_search_save_order', { order: [{ id: 'muniment', enabled: true }, { id: 'exa', enabled: true }, { id: 'tavily', enabled: true }] })
  expect(invoke.mock.calls.filter(([command]) => command === 'web_search_save_order')).toHaveLength(1)
})

it('saves the order with a search turned off', async () => {
  const invoke = setup({ web_search_save_order: ({ order }) => settings({ order: order.map(({ id, enabled }) => [id, enabled]) }) })
  await fireEvent.click(await screen.findByRole('switch', { name: 'Use Tavily' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('web_search_save_order', { order: [{ id: 'muniment', enabled: true }, { id: 'tavily', enabled: false }, { id: 'exa', enabled: true }] }))
})

it('saves the enabled engines when one turns off', async () => {
  const invoke = setup({ web_search_save_engines: ({ engines }) => settings({ engines }) })
  await waitFor(() => expect(screen.getByRole('switch', { name: 'Search Bing' })).toBeEnabled())
  await fireEvent.click(screen.getByRole('switch', { name: 'Search Bing' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('web_search_save_engines', { engines: ['brave', 'duckduckgo', 'wikipedia', 'yahoo'] }))
  expect(screen.getByRole('switch', { name: 'Search Bing' })).toHaveAttribute('aria-checked', 'false')
})

it('shows the runtime refusal and restores the engines it kept', async () => {
  const invoke = setup({ web_search_save_engines: () => { throw 'Turn on at least one engine, or turn off Muniment search in the order.' } })
  await waitFor(() => expect(screen.getByRole('switch', { name: 'Search Bing' })).toBeEnabled())
  await fireEvent.click(screen.getByRole('switch', { name: 'Search Bing' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('Turn on at least one engine')
  await waitFor(() => expect(screen.getByRole('switch', { name: 'Search Bing' })).toHaveAttribute('aria-checked', 'true'))
  expect(invoke.mock.calls.filter(([command]) => command === 'web_search_settings')).toHaveLength(2)
})

it('saves a key with its server URL and clears the key input', async () => {
  const saved = settings({ order: [['muniment', true], ['tavily', true], ['exa', true], ['firecrawl', true]], keys: [{ provider: 'tavily', has_key: true, last4: '3f2a' }, { provider: 'firecrawl', has_key: true, last4: '9c1d', base_url: 'https://crawl.example' }] })
  const invoke = setup({ web_search_set_key: () => saved })
  const add = (await screen.findByRole('heading', { name: 'Add a search provider' })).closest('section')
  await waitFor(() => expect(within(add).getByText('Firecrawl', { selector: 'strong' })).toBeInTheDocument())
  const row = within(add).getByText('Firecrawl', { selector: 'strong' }).closest('.row')
  await fireEvent.click(within(row).getByRole('button', { name: 'Add key' }))
  await fireEvent.input(screen.getByLabelText('Firecrawl server URL'), { target: { value: 'https://crawl.example' } })
  const key = screen.getByLabelText('Firecrawl API key')
  await fireEvent.input(key, { target: { value: 'fc-test-key-9c1d' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('web_search_set_key', { provider: 'firecrawl', apiKey: 'fc-test-key-9c1d', baseUrl: 'https://crawl.example', extra: null }))
  await waitFor(() => expect(orderNames()).toEqual(['Muniment search', 'Tavily', 'Exa free', 'Firecrawl']))
  expect(key).toHaveValue('')
  expect(screen.queryByLabelText('Firecrawl API key')).not.toBeInTheDocument()
  expect(screen.getByText('••••9c1d')).toBeInTheDocument()
})

it('asks for the Bright Data zones the runtime names', async () => {
  const invoke = setup({ web_search_set_key: () => settings() })
  await fireEvent.click(await screen.findByText('More providers (1)'))
  await fireEvent.click(screen.getByText('Bright Data').closest('li').querySelector('button'))
  await fireEvent.input(screen.getByLabelText('Bright Data SERP zone'), { target: { value: 'serp_zone' } })
  await fireEvent.input(screen.getByLabelText('Bright Data API key'), { target: { value: 'bd-test-key-0001' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('web_search_set_key', { provider: 'brightdata', apiKey: 'bd-test-key-0001', baseUrl: null, extra: { brightdataSerpZone: 'serp_zone', brightdataUnlockerZone: '' } }))
})

it('reopens the key form from the edit control', async () => {
  setup()
  await fireEvent.click(await screen.findByRole('button', { name: 'Edit Tavily key' }))
  expect(screen.getByLabelText('Tavily API key')).toHaveValue('')
})

it('removes a provider key', async () => {
  const invoke = setup({ web_search_remove_key: () => settings({ order: [['muniment', true], ['exa', true]], keys: [] }) })
  await fireEvent.click(await screen.findByRole('button', { name: 'Remove Tavily' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('web_search_remove_key', { provider: 'tavily' }))
  await waitFor(() => expect(orderNames()).toEqual(['Muniment search', 'Exa free']))
})

it('warns when every search is off', async () => {
  const invoke = vi.fn(async () => settings({ order: [['muniment', false], ['exa', false]], keys: [] }))
  render(SearchSection, { tauri: { invoke } })
  expect(await screen.findByText('Every search is off, so agents cannot search the web.')).toBeInTheDocument()
})

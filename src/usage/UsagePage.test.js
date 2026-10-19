import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import UsagePage from './UsagePage.svelte'
import LimitBanner from './LimitBanner.svelte'
import { machineRows } from '../runtimes/machines.js'
import { accountLine, composerLimit, compactTokens, dollars, isUsageShortcut, machineRefs, providerLabel, resetsIn, rowLabel, totalTiles, windowLabel } from './usage.js'

afterEach(cleanup)

const NOW = Date.UTC(2026, 9, 17, 12)
const HOUR = 3_600_000
const limit = (extra = {}) => ({ provider: 'anthropic', kind: 'five_hour', scope: '', remaining_percent: 62, resets_at_ms: NOW + 2 * HOUR + 14 * 60_000, elapsed_percent: 40, pace: 'ahead', status: 'ok', account_id: 'a', label: 'sam@example.com', plan: 'max', ...extra })
const amount = (processed, cost) => ({ tokens: { input: 1, output: 1, cache_read: 1, cache_write: 0, reasoning: 0 }, processed, runs: 2, cost_usd: cost, unpriced_runs: 0 })
const summary = (extra = {}) => ({
  range: '7d', bucket: 'day',
  totals: { processed: 15_300_000, cached_input: 9_500_000, uncached_input: 4_600_000, output: 1_200_000, cache_hit_percent: 67.4, runs: 50, cost_usd: 12.5, unpriced_runs: 3 },
  series: [{ start_ms: NOW - 24 * HOUR, providers: { anthropic: amount(900, 0.2) } }, { start_ms: NOW, providers: { anthropic: amount(300, 0.1), openai: amount(600, 0.3) } }],
  breakdown: { model: [{ key: 'anthropic/claude', label: 'claude', provider: 'anthropic', ...amount(1200, 0.3) }], agent: [{ key: '', label: '', ...amount(1200, 0.3) }], project: [], machine: [{ key: 'local', label: 'This computer', ...amount(1200, 0.3) }] },
  providers: ['anthropic', 'openai'],
  limits: [limit(), limit({ account_id: 'b', label: 'Home', plan: null, remaining_percent: 30 }), limit({ provider: 'openai', kind: 'weekly', remaining_percent: 0, status: 'limit', pace: null, elapsed_percent: null })],
  unmetered: [{ provider: 'ollama', label: 'Ollama', reason: 'local' }, { provider: 'xai', label: 'xAI', reason: 'pending' }],
  windows: [],
  machines: [{ id: 'local', name: 'This computer', state: 'ok' }, { id: 'box', name: 'Build box', state: 'error', error: 'offline' }],
  partial: true,
  ...extra,
})
const runtimeList = { runtimes: [{ id: 'local', local: true }, { id: 'box', name: 'Build box', status: 'online' }, { id: 'studio', name: 'Studio', status: 'offline' }] }

describe('usage words', () => {
  it('names windows, resets, tokens and dollars', () => {
    expect(windowLabel({ kind: 'five_hour' })).toBe('5-hour')
    expect(windowLabel({ kind: 'weekly', scope: 'Fable' })).toBe('Weekly Fable')
    expect(resetsIn(NOW + 2 * HOUR + 14 * 60_000, NOW)).toBe('Resets in 2h 14m')
    expect(resetsIn(NOW + 30 * 60_000, NOW)).toBe('Resets in 30m')
    expect(resetsIn(NOW + 50 * HOUR, NOW)).toBe('Resets in 2d 2h')
    expect(compactTokens(950)).toBe('950')
    expect(compactTokens(12_400)).toBe('12.4K')
    expect(compactTokens(3_100_000)).toBe('3.1M')
    expect(dollars(0.004)).toBe('<$0.01')
    expect(dollars(1234.5)).toBe('$1,234.50')
    expect(accountLine({ label: 'sam@example.com', plan: 'max' })).toBe('sam@example.com · max')
    expect(accountLine({})).toBe('')
    expect(rowLabel({ key: '' }, 'agent')).toBe('No agent')
    expect(rowLabel({ key: '' }, 'project')).toBe('No project')
    expect(providerLabel('anthropic')).toBe('Anthropic')
    expect(totalTiles(summary().totals, 'cost')[0]).toEqual({ label: 'API estimate', value: '$12.50' })
    expect(totalTiles(summary().totals, 'tokens').map((tile) => tile.label)).toEqual(['Processed', 'Cached input', 'Uncached input', 'Output', 'Cache hit rate'])
  })

  it('asks this computer and each paired machine that answers', () => {
    expect(machineRefs(runtimeList)).toEqual([{ id: 'local', name: 'This computer' }, { id: 'box', name: 'Build box' }])
  })

  it('names each machine as the Machines list does', () => {
    const list = { runtimes: [{ id: 'local', local: true, name: 'studio' }, { id: 'pod', name: 'Muniment runtime', kind: 'container', status: 'online' }, { id: 'box', name: 'Build box', kind: 'server', status: 'online' }] }
    const rows = Object.fromEntries(machineRows({ list }).map((row) => [row.key, row.name]))
    expect(machineRefs(list)).toEqual([{ id: 'local', name: 'This computer' }, { id: 'pod', name: 'Container' }, { id: 'box', name: 'Build box' }])
    expect(machineRefs(list).map((ref) => ref.name)).toEqual(list.runtimes.map((runtime) => rows[runtime.id]))
  })

  it('warns the composer only at or near a limit, for the provider in use', () => {
    const low = limit({ provider: 'openai', status: 'low', remaining_percent: 8 })
    const lowish = limit({ status: 'low', remaining_percent: 15 })
    const spent = limit({ provider: 'google', status: 'limit', remaining_percent: 0 })
    expect(composerLimit([lowish])).toBeNull()
    expect(composerLimit([lowish, low])).toBe(low)
    expect(composerLimit([low, spent])).toBe(spent)
    expect(composerLimit([low, spent], 'openai')).toBe(low)
    expect(composerLimit([low, spent], 'anthropic')).toBeNull()
  })

  it('takes Super+Shift+U', () => {
    expect(isUsageShortcut({ key: 'U', metaKey: true, shiftKey: true }, 'MacIntel')).toBe(true)
    expect(isUsageShortcut({ key: 'u', metaKey: true }, 'MacIntel')).toBe(false)
    expect(isUsageShortcut({ key: 'u', ctrlKey: true, shiftKey: true }, 'Win32')).toBe(true)
  })
})

describe('Usage page', () => {
  it('shows a card per account and window with a threshold mark', async () => {
    const invoke = vi.fn(async () => summary())
    render(UsagePage, { tauri: { invoke }, runtimeList })
    const card = await screen.findByRole('article', { name: 'Anthropic · 5-hour, sam@example.com' })
    expect(invoke).toHaveBeenCalledWith('usage_summary', { machines: [{ id: 'local', name: 'This computer' }, { id: 'box', name: 'Build box' }], body: expect.objectContaining({ range: '7d' }) })
    expect(within(card).getByText('62% left')).toBeInTheDocument()
    expect(within(card).getByRole('meter')).toHaveAttribute('aria-valuenow', '62')
    expect(within(card).getByText('sam@example.com · max')).toBeInTheDocument()
    // A second account of one provider has its own card.
    const second = screen.getByRole('article', { name: 'Anthropic · 5-hour, Home' })
    expect(within(second).getByText('30% left')).toBeInTheDocument()
    expect(within(screen.getByRole('article', { name: 'OpenAI · Weekly, sam@example.com' })).getByText('Limit reached')).toBeInTheDocument()
    expect(screen.getByText('No limit')).toBeInTheDocument()
    expect(screen.getByText('Allowance pending')).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Build box did not answer. The totals leave it out.')
  })

  it('says when a weekly window has no 5-hour window beside it', async () => {
    const weekly = limit({ provider: 'openai', kind: 'weekly', label: '', note: 'The plan reports no 5-hour window.' })
    render(UsagePage, { tauri: { invoke: vi.fn(async () => summary({ limits: [weekly] })) }, runtimeList })
    const card = await screen.findByRole('article', { name: 'OpenAI · Weekly' })
    expect(within(card).getByText('The plan reports no 5-hour window.')).toBeInTheDocument()
  })

  it('shows tokens and cost with a breakdown, and asks again for a range or a machine', async () => {
    const invoke = vi.fn(async () => summary())
    render(UsagePage, { tauri: { invoke }, runtimeList })
    await screen.findByRole('article', { name: 'Anthropic · 5-hour, sam@example.com' })
    await fireEvent.click(screen.getByRole('radio', { name: 'Tokens' }))
    expect(screen.getByText('15.3M')).toBeInTheDocument()
    expect(screen.getAllByRole('img').some((bar) => bar.getAttribute('aria-label')?.includes('OpenAI'))).toBe(true)
    expect(screen.getByRole('list', { name: 'Usage by model' })).toHaveTextContent('claude')
    await fireEvent.click(screen.getByRole('radio', { name: 'Agent' }))
    expect(screen.getByRole('list', { name: 'Usage by agent' })).toHaveTextContent('No agent')
    await fireEvent.click(screen.getByRole('radio', { name: 'Cost' }))
    expect(screen.getByText('API estimate', { selector: 'dt' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('radio', { name: '30d' }))
    await vi.waitFor(() => expect(invoke).toHaveBeenLastCalledWith('usage_summary', expect.objectContaining({ body: expect.objectContaining({ range: '30d' }) })))
  })

  it('asks each account again on Refresh, then reads usage', async () => {
    const invoke = vi.fn(async (command) => command === 'usage_summary' ? summary() : { accounts: [{ id: 'a', allowance_readable: true }] })
    render(UsagePage, { tauri: { invoke }, runtimeList })
    await screen.findByRole('article', { name: 'Anthropic · 5-hour, sam@example.com' })
    const reads = () => invoke.mock.calls.filter(([command]) => command === 'usage_summary').length
    const before = reads()
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh usage' }))
    await vi.waitFor(() => expect(reads()).toBe(before + 1))
    expect(invoke).toHaveBeenCalledWith('model_router_refresh_quota', { id: null })
  })

  it('names the failure when no machine answers', async () => {
    render(UsagePage, { tauri: { invoke: vi.fn(async () => { throw 'The background service is not running.' }) }, runtimeList })
    expect(await screen.findByRole('alert')).toHaveTextContent('The background service is not running.')
  })
})

describe('limit banner', () => {
  it('names the window and the reset', async () => {
    const onview = vi.fn()
    render(LimitBanner, { limit: limit({ provider: 'openai', kind: 'weekly', status: 'limit', remaining_percent: 0 }), now: NOW, onview })
    expect(screen.getByRole('region', { name: 'Usage limit' })).toHaveTextContent('OpenAI weekly limit reached. Resets in 2h 14m.')
    await fireEvent.click(screen.getByRole('button', { name: 'View usage' }))
    expect(onview).toHaveBeenCalledOnce()
  })
})

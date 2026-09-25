import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
// Compile the lazy section before the test measures UI readiness.
import './ModelsSection.svelte'
import Settings from './Settings.svelte'
import ModelAccounts from './ModelAccounts.svelte'
import RoutingTest from './RoutingTest.svelte'

const inventory = { providers: [], hidden: [] }
const routing = () => ({
  enabled: true, running: true, is_default: false, base_url: 'http://127.0.0.1:1234/v1',
  accounts: [], subscriptions: [], families: [], options: [], fallback: null,
  min_confidence: 0.6, classifier: { kind: 'none', configured: false, model: '' },
})

afterEach(() => { cleanup(); vi.useRealTimers() })

describe('Routing settings', () => {
  it('separates accounts, models and routing on one settings screen', async () => {
    const tauri = { invoke: vi.fn(async (command) => command === 'model_router_settings' ? routing() : inventory) }
    render(Settings, { tauri, inventory, section: 'models', onclose: vi.fn() })
    await screen.findByRole('button', { name: 'Connect account' })
    expect(screen.queryByRole('searchbox', { name: 'Search models' })).toBeNull()
    await fireEvent.click(screen.getByRole('tab', { name: 'Models', exact: true }))
    expect(screen.getByRole('searchbox', { name: 'Search models' })).toBeInTheDocument()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh models' })).toBeEnabled())
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh models' }))
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('local_mode_provider_inventory', { force: true }))
    expect(screen.queryByRole('button', { name: 'Connect account' })).toBeNull()
    await fireEvent.click(screen.getByRole('tab', { name: 'Routing', exact: true }))
    expect(await screen.findByRole('switch', { name: 'Use routing' })).toBeDisabled()
    expect(screen.queryByText('Classifier and fallback')).toBeNull()
    expect(screen.queryByText('Test routing', { selector: 'summary' })).toBeNull()

  })

  it('retries failed reads without displaying an empty pool as fact', async () => {
    let failed = true
    const tauri = { invoke: vi.fn(async (command) => {
      if (command !== 'model_router_settings') return inventory
      if (failed) throw new Error('offline')
      return routing()
    }) }
    render(Settings, { tauri, inventory, section: 'routing', onclose: vi.fn() })
    await screen.findByRole('alert')
    expect(screen.queryByText('Connect an account to choose models automatically.')).not.toBeInTheDocument()
    failed = false
    await fireEvent.click(screen.getByRole('button', { name: 'Retry routing settings' }))
    await screen.findByText('Connect an account to choose models automatically.')
  })

  it('enables routing with a connected classifier and keeps balancing on', async () => {
    const settings = routing()
    settings.enabled = false
    settings.options = [{ key: 'openai/example', family: 'openai', model: 'example' }]
    settings.classifier_connections = [{ id: 'jev', name: 'Jev via TypeSafe', active: false }]
    let fresh = { ...inventory, default_provider: 'openai', default_model: 'example' }
    const tauri = { invoke: vi.fn(async (command, payload) => {
      if (command === 'local_mode_provider_inventory') return fresh
      if (command === 'model_router_set_enabled') settings.enabled = payload.enabled
      if (command === 'model_router_select_classifier') settings.classifier_connections[0].active = true
      if (command === 'local_mode_set_default_model') fresh = { ...fresh, default_provider: payload.provider, default_model: payload.model }
      return { ...settings }
    }) }
    render(Settings, { tauri, inventory: fresh, section: 'routing', onclose: vi.fn() })
    const toggle = await screen.findByRole('switch', { name: 'Use routing' })
    expect(screen.queryByRole('button', { name: /^Classifier model:/ })).toBeNull()
    await fireEvent.click(toggle)
    await screen.findByRole('button', { name: /^Classifier model:/ })
    expect(tauri.invoke).toHaveBeenCalledWith('model_router_select_classifier', { id: 'jev' })
    expect(tauri.invoke).toHaveBeenCalledWith('model_router_set_enabled', { enabled: true })
    expect(tauri.invoke).toHaveBeenCalledWith('local_mode_set_default_model', { provider: 'muniment-router', model: 'auto' })
  })

})

it('does not present connected subscriptions or zero-weight keys as ready', () => {
  const account = { family: 'openai', label: 'Work account', source: 'subscription', enabled: true, weight: 1, models: [], days: [], windows: [], requests: 0, input_tokens: 0, output_tokens: 0, active: 0, errors: 0 }
  render(ModelAccounts, { tauri: { invoke: vi.fn() }, family: 'openai', settings: {
    ...routing(), accounts: [{ ...account, id: 'subscription', servable: false }, { ...account, id: 'key', label: 'API account', source: 'key', weight: 0 }],
  } })
  expect(screen.getByText('Routing unavailable')).toBeInTheDocument()
  expect(screen.getByText('Excluded from routing')).toBeInTheDocument()
  expect(screen.queryByText('Available for routed turns.')).not.toBeInTheDocument()
})

it('refreshes allowances on load and every two minutes, then stops when closed', async () => {
  const settings = { ...routing(), accounts: [{ id: 'a', family: 'openai', label: 'Test account', source: 'account', enabled: true, servable: true, weight: 1, models: [], days: [], windows: [], allowance_readable: true }] }
  const tauri = { invoke: vi.fn(async (command) => command === 'local_mode_provider_inventory' ? inventory : settings) }
  const view = render(Settings, { tauri, inventory, section: 'models', onclose: vi.fn() })
  await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('model_router_refresh_quota', { id: null }))
  vi.useFakeTimers()
  // Remount under the controlled clock to check the interval and its cleanup.
  view.unmount()
  const next = render(Settings, { tauri, inventory, section: 'models', onclose: vi.fn() })
  await vi.advanceTimersByTimeAsync(0)
  const calls = () => tauri.invoke.mock.calls.filter(([command]) => command === 'model_router_refresh_quota').length
  const initial = calls()
  await vi.advanceTimersByTimeAsync(119_999)
  expect(calls()).toBe(initial)
  await vi.advanceTimersByTimeAsync(1)
  expect(calls()).toBe(initial + 1)
  next.unmount()
  await vi.advanceTimersByTimeAsync(240_000)
  expect(calls()).toBe(initial + 1)
})

it('shows allowances and manual refresh for routable subscriptions', async () => {
  render(ModelAccounts, { tauri: { invoke: vi.fn() }, family: 'openai', settings: { ...routing(), accounts: [{ id: 'a', family: 'openai', label: 'Test account', source: 'account', servable: true, enabled: true, weight: 1, models: [], days: [], allowance_readable: true, quota_observed_ms: Date.now(), windows: [{ label: 'Weekly', scope: '', remaining_percent: 70, resets_at_ms: null, limit_reached: false }], requests: 0, input_tokens: 0, output_tokens: 0, active: 0, errors: 0 }] } })
  expect(screen.getByText('70%')).toBeInTheDocument()
  expect(screen.queryByText('Available for routed turns.')).not.toBeInTheDocument()
  await fireEvent.click(screen.getByText('Usage and settings'))
  expect(screen.getByRole('button', { name: 'Refresh allowance' })).toBeInTheDocument()
})

it('shows runtime routing eligibility and fallback evidence without starting a chat', async () => {
  const tauri = { invoke: vi.fn(async () => ({ model: 'openai/ready', reason: 'Fallback used because the classifier failed', confidence: null, elapsed_ms: 35, eligible_models: ['openai/ready'], exclusions: [{ model: 'anthropic/cooling', reason: 'Every account for this provider is temporarily unavailable.' }], fallback_reason: 'The classifier did not return a valid choice.' })) }
  render(RoutingTest, { tauri, settings: { enabled: true, options: [{ key: 'openai/ready' }] } })
  await fireEvent.input(screen.getByLabelText('Sample request'), { target: { value: 'Summarize my file' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Test routing' }))
  await screen.findByText('The classifier did not return a valid choice.')
  expect(screen.getByText('Anthropic · Cooling')).toBeInTheDocument()
  expect(screen.getByText('Every account for this provider is temporarily unavailable.')).toBeInTheDocument()
  expect(screen.getByText('35 ms')).toBeInTheDocument()
  expect(tauri.invoke.mock.calls).toEqual([['model_router_test_route', { sample: 'Summarize my file' }]])
})

import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelsSection from './ModelsSection.svelte'
afterEach(cleanup)

const inventory = {
  providers: [
    { id: 'custom-omlx', name: 'oMLX', source: 'custom', base_url: 'http://127.0.0.1:8080/v1', models: [{ id: 'Qwen3.8-27B-4bit' }, { id: 'gemma-4-26b-a4b-it-4bit' }] },
    { id: 'custom-empty', name: 'Spare box', source: 'custom', base_url: 'http://10.0.0.2:4000/v1', models: [] },
  ],
  hidden: [], shown: [], default_provider: 'custom-omlx', default_model: 'Qwen3.8-27B-4bit',
}

function bridge() {
  return { invoke: vi.fn(async (command) => {
    if (command === 'local_mode_provider_inventory') return inventory
    if (command === 'model_router_settings') return { accounts: [], subscriptions: [], families: [] }
    if (command === 'fleet_accounts') return null
    return null
  }) }
}

it('names how many models each endpoint serves and what Refresh models found', async () => {
  const tauri = bridge()
  render(ModelsSection, { props: { tauri, listen: async () => () => {}, inventory } })
  const omlx = screen.getByRole('region', { name: 'oMLX' })
  expect(within(omlx).getByText('2 models')).toBeInTheDocument()
  expect(within(screen.getByRole('region', { name: 'Spare box' })).getByText('No models')).toBeInTheDocument()
  await fireEvent.click(await screen.findByRole('button', { name: 'Refresh models' }))
  expect(await screen.findByText('Models refreshed. 2 models from 1 provider.')).toBeInTheDocument()
  expect(tauri.invoke).toHaveBeenCalledWith('local_mode_provider_inventory', { force: true })
})

it('offers the servers found on this computer under Connect account and connects one in one step', async () => {
  const scan = { servers: [
    { id: 'omlx', name: 'oMLX', baseUrl: 'http://127.0.0.1:8000/v1', state: 'needs-key', modelCount: 6, keyFile: '~/.omlx/settings.json' },
    { id: 'lmstudio', name: 'LM Studio', baseUrl: 'http://127.0.0.1:1234/v1', state: 'needs-key', modelCount: 2 },
    // Already connected, so it is not offered again.
    { id: 'omlx-other', name: 'Connected one', baseUrl: 'http://127.0.0.1:8080/v1', state: 'ready', modelCount: 2 },
  ], hints: [] }
  const tauri = bridge()
  const base = tauri.invoke.getMockImplementation()
  tauri.invoke.mockImplementation(async (command, payload) => command === 'onboarding_provider_scan' ? scan : base(command, payload))
  render(ModelsSection, { props: { tauri, listen: async () => () => {}, inventory } })
  await fireEvent.click(screen.getByRole('button', { name: 'Connect account' }))
  const found = await screen.findByRole('region', { name: 'Found on this computer' })
  expect(found).toHaveTextContent('oMLX')
  expect(found).not.toHaveTextContent('Connected one')
  // A server without a key file opens the custom endpoint form with its name and address.
  await fireEvent.click(within(found).getByRole('button', { name: /LM Studio/ }))
  expect(screen.getByLabelText('Name')).toHaveValue('LM Studio')
  expect(screen.getByLabelText('Server URL')).toHaveValue('http://127.0.0.1:1234/v1')
  await fireEvent.click(screen.getByRole('button', { name: 'Back' }))
  await fireEvent.click(within(await screen.findByRole('region', { name: 'Found on this computer' })).getByRole('button', { name: /oMLX/ }))
  expect(tauri.invoke).toHaveBeenCalledWith('onboarding_connect_server', { id: 'omlx', baseUrl: 'http://127.0.0.1:8000/v1', key: null, keyFromFile: true })
  expect(await screen.findByText('Muniment connected oMLX.')).toBeInTheDocument()
})

it('shows each Anthropic account Claude Code signed in to as its own card, and adds another through the same sign-in', async () => {
  const claudeAccount = (id, email) => ({ id, family: 'anthropic', label: email, email, source: 'account', pi_provider: 'claude-bridge', plan: 'max', enabled: true, servable: true, weight: 1, models: [], days: [], windows: [], allowance_readable: true, requests: 0, input_tokens: 0, output_tokens: 0, active: 0, errors: 0 })
  const router = {
    families: [{ id: 'anthropic', name: 'Anthropic', base_url: 'https://api.anthropic.com/v1' }],
    subscriptions: [{ provider: 'anthropic', family: 'anthropic', label: 'Claude Pro or Max' }],
    accounts: [claudeAccount('a1', 'work@example.test'), claudeAccount('a2', 'home@example.test')],
  }
  const claude = { providers: [{ id: 'claude-bridge', name: 'Anthropic', source: 'account', base_url: null, models: [{ id: 'claude-opus-5' }] }], hidden: [], shown: [], default_provider: null, default_model: null }
  const tauri = { invoke: vi.fn(async (command) => {
    if (command === 'local_mode_provider_inventory') return claude
    if (command === 'model_router_settings' || command === 'model_router_remove_account') return router
    return null
  }) }
  render(ModelsSection, { props: { tauri, listen: async () => () => {}, inventory: claude } })
  const group = await screen.findByRole('region', { name: 'Anthropic' })
  expect(await within(group).findByText('work@example.test')).toBeInTheDocument()
  expect(within(group).getByText('home@example.test')).toBeInTheDocument()
  // The accounts are the cards. No Claude Code connection row stands over them.
  expect(within(group).queryByTestId('provider-connection')).not.toBeInTheDocument()
  await fireEvent.click(within(group).getByRole('button', { name: 'Disconnect home@example.test' }))
  expect(tauri.invoke).toHaveBeenCalledWith('model_router_remove_account', { id: 'a2' })
  await fireEvent.click(within(group).getByRole('button', { name: 'Subscription: Claude Pro or Max' }))
  expect(tauri.invoke).toHaveBeenCalledWith('model_router_subscription_start', { provider: 'anthropic' })
})

it('shows a Claude Code connection from before as one row that signs in again as an account', async () => {
  const older = { providers: [{ id: 'claude-bridge', name: 'Anthropic', source: 'claude-code', base_url: null, models: [{ id: 'claude-opus-5' }] }], hidden: [], shown: [], default_provider: null, default_model: null }
  const tauri = { invoke: vi.fn(async (command) => {
    if (command === 'local_mode_provider_inventory') return older
    if (command === 'model_router_settings') return { accounts: [], subscriptions: [], families: [] }
    return null
  }) }
  render(ModelsSection, { props: { tauri, listen: async () => () => {}, inventory: older } })
  const group = await screen.findByRole('region', { name: 'Anthropic' })
  const row = within(group).getByTestId('provider-connection')
  expect(row).toHaveTextContent('Claude Code')
  expect(within(row).queryByRole('button', { name: 'Edit Anthropic' })).not.toBeInTheDocument()
  await fireEvent.click(within(row).getByRole('button', { name: 'Sign in again' }))
  expect(tauri.invoke).toHaveBeenCalledWith('local_mode_account_login_start', { provider: 'anthropic' })
  expect(await screen.findByRole('heading', { name: 'Connect Anthropic' })).toBeInTheDocument()
})

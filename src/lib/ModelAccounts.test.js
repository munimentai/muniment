import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelAccounts from './ModelAccounts.svelte'
import Settings from './Settings.svelte'

afterEach(cleanup)

const STUDIO = '018f0000-0000-7000-8000-0000000000aa'
function subscription(id, label) {
  return { id, label, family: 'openai', source: 'subscription', plan: 'Pro', enabled: true, servable: true, weight: 1, allowance_readable: true, windows: [], quota_observed_ms: 1, models: [], days: [], requests: 0, active: 0, errors: 0, input_tokens: 0, output_tokens: 0, last_used_ms: null }
}
const settings = { accounts: [subscription('work', 'Work'), subscription(`shared:swarm:${STUDIO}:team`, 'Team')], subscriptions: [], families: [{ id: 'openai', name: 'OpenAI' }] }
const fleet = [
  { name: 'router:work', kind: 'subscription', holder: 'self', holder_name: 'Laptop', shared: true, this_machine: true, usable_here: true },
  { name: 'router:team', kind: 'subscription', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: true },
]
const card = (label) => screen.getByText(label, { selector: '.card-label' }).closest('li')

describe('Hold sign-in here', () => {
  it('names the holder of each shared sign-in and moves one here through fleet.hold_sign_in', async () => {
    const invoke = vi.fn(async (command, payload) => {
      if (command === 'agent_request' && payload.operation === 'fleet.hold_sign_in') return { status: 'requested', holder: STUDIO }
      throw new Error(`unexpected command: ${command}`)
    })
    const onheld = vi.fn()
    render(ModelAccounts, { tauri: { invoke }, settings, family: 'openai', fleet, onheld })

    expect(within(card('Work')).getByText('Signed in here')).toBeInTheDocument()
    expect(within(card('Work')).queryByRole('button', { name: 'Hold sign-in here' })).toBeNull()
    const team = card('Team')
    expect(within(team).getByText('Signed in on Studio')).toBeInTheDocument()

    await fireEvent.click(within(team).getByRole('button', { name: 'Hold sign-in here' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'fleet.hold_sign_in', body: { account: 'router:team' } }))
    expect(await within(team).findByText('Moving the sign-in here.')).toBeInTheDocument()
    expect(within(team).queryByRole('button', { name: 'Hold sign-in here' })).toBeNull()
    expect(onheld).toHaveBeenCalledWith('requested')
  })

  it('names the failure and keeps the control when core refuses', async () => {
    const invoke = vi.fn(async () => { throw new Error('Studio does not answer. Try again when it is online.') })
    render(ModelAccounts, { tauri: { invoke }, settings, family: 'openai', fleet })
    await fireEvent.click(within(card('Team')).getByRole('button', { name: 'Hold sign-in here' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Studio does not answer.')
    expect(within(card('Team')).getByText('Signed in on Studio')).toBeInTheDocument()
    expect(within(card('Team')).getByRole('button', { name: 'Hold sign-in here' })).toBeEnabled()
  })

  it('shows no holder and no control without the fleet accounts', () => {
    render(ModelAccounts, { tauri: { invoke: vi.fn() }, settings, family: 'openai', fleet: null })
    expect(screen.queryByText(/^Signed in/)).toBeNull()
    expect(screen.queryByRole('button', { name: 'Hold sign-in here' })).toBeNull()
  })
})

describe('Disconnect', () => {
  it('asks in the app dialog before it removes a key, and keeps the key on Cancel', async () => {
    const key = { ...subscription('k1', 'invalid-test'), source: 'key' }
    const invoke = vi.fn(async () => ({ ...settings, accounts: [] }))
    render(ModelAccounts, { tauri: { invoke }, settings: { ...settings, accounts: [key] }, family: 'openai', fleet: null, onsettings: vi.fn() })
    await fireEvent.click(screen.getByRole('button', { name: 'Disconnect invalid-test' }))
    const dialog = await screen.findByRole('dialog', { name: 'Disconnect the OpenAI key invalid-test?' })
    expect(dialog).toHaveTextContent('Muniment deletes this key.')
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(invoke).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: 'Disconnect invalid-test' }))
    await fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Disconnect' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('model_router_remove_account', { id: 'k1' }))
  })

  it('names the provider of a sign-in whose email another provider shares', async () => {
    const shared = { ...subscription('s1', 'support@example.test'), source: 'subscription' }
    render(ModelAccounts, { tauri: { invoke: vi.fn() }, settings: { ...settings, accounts: [shared] }, family: 'openai', fleet: null })
    await fireEvent.click(screen.getByRole('button', { name: 'Disconnect support@example.test' }))
    expect(await screen.findByRole('dialog', { name: 'Disconnect the OpenAI account support@example.test?' })).toBeInTheDocument()
  })

  it('keeps a key name as typed, with no automatic capital', async () => {
    render(ModelAccounts, { tauri: { invoke: vi.fn() }, settings, family: 'openai', fleet: null })
    await fireEvent.click(screen.getByRole('button', { name: 'Add a key' }))
    const name = screen.getByLabelText('Name')
    expect(name).toHaveAttribute('autocapitalize', 'off')
    expect(name).toHaveAttribute('spellcheck', 'false')
  })
})

describe('the sign-in location', () => {
  it('names a Claude Code account this machine holds as signed in here, like a shared one', () => {
    const claude = { ...subscription('c1', 'me@example.test'), family: 'anthropic', pi_provider: 'claude-bridge' }
    render(ModelAccounts, { tauri: { invoke: vi.fn() }, settings: { ...settings, accounts: [claude], families: [{ id: 'anthropic', name: 'Anthropic' }] }, family: 'anthropic', fleet })
    expect(within(card('me@example.test')).getByText('Signed in here')).toBeInTheDocument()
  })
})

describe('a refused sign-in', () => {
  it('says the sign-in expired in place of a rate limit until a turn succeeds', () => {
    const expired = { ...subscription('work', 'Work'), credential_refused: true, cooldown_until_ms: Date.now() + 60_000 }
    render(ModelAccounts, { tauri: { invoke: vi.fn() }, settings: { ...settings, accounts: [expired] }, family: 'openai', fleet: null })
    expect(within(card('Work')).getByText('Sign-in expired. Sign in again to use this account.')).toBeInTheDocument()
    expect(within(card('Work')).queryByText(/Rate limited/)).toBeNull()
  })
})

describe('sign-ins on other machines', () => {
  it('lists each sign-in another machine holds that this machine cannot use yet, with no hold control', async () => {
    const inventory = { providers: [], hidden: [] }
    const router = { enabled: true, running: true, is_default: false, base_url: 'http://127.0.0.1:1234/v1', accounts: [], subscriptions: [], families: [], options: [], fallback: null, min_confidence: 0.6, classifier: { kind: 'none', configured: false, model: '' } }
    const accounts = [
      { name: 'pi:anthropic', provider: 'anthropic', kind: 'oauth', label: 'anthropic', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: false },
      { name: 'router:home', provider: 'openai', family: 'openai', kind: 'subscription', label: 'Home', holder: STUDIO, holder_name: 'Studio', shared: false, this_machine: false, usable_here: false },
      { name: 'router:key', provider: 'openai', family: 'openai', kind: 'api_key', label: 'Key', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: false },
    ]
    const invoke = vi.fn(async (command, payload) => {
      if (command === 'agent_request' && payload.operation === 'fleet.credentials') return { share_models: true, accounts }
      if (command === 'model_router_settings') return router
      return inventory
    })
    await import('./ModelsSection.svelte')
    render(Settings, { tauri: { invoke }, inventory, section: 'models', onclose: vi.fn() })
    const group = (await screen.findByRole('heading', { name: 'Signed in on other machines' })).closest('section')
    expect(within(group).getByText('anthropic')).toBeInTheDocument()
    expect(within(group).getByText('Signed in on Studio, arriving')).toBeInTheDocument()
    expect(within(group).getByText('Home')).toBeInTheDocument()
    expect(within(group).getByText('Signed in on Studio, not shared')).toBeInTheDocument()
    expect(within(group).queryByText('Key')).toBeNull()
    expect(within(group).queryByRole('button', { name: 'Hold sign-in here' })).toBeNull()
  })
})

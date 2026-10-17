import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelAccounts from './ModelAccounts.svelte'

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

    expect(within(card('Work')).getByText('Held here')).toBeInTheDocument()
    expect(within(card('Work')).queryByRole('button', { name: 'Hold sign-in here' })).toBeNull()
    const team = card('Team')
    expect(within(team).getByText('Held by Studio')).toBeInTheDocument()

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
    expect(within(card('Team')).getByText('Held by Studio')).toBeInTheDocument()
    expect(within(card('Team')).getByRole('button', { name: 'Hold sign-in here' })).toBeEnabled()
  })

  it('shows no holder and no control without the fleet accounts', () => {
    render(ModelAccounts, { tauri: { invoke: vi.fn() }, settings, family: 'openai', fleet: null })
    expect(screen.queryByText(/^Held/)).toBeNull()
    expect(screen.queryByRole('button', { name: 'Hold sign-in here' })).toBeNull()
  })
})

import { describe, expect, it, vi } from 'vitest'
import { holdLine, otherSignIns, piHold, readFleetAccounts, routerHold } from './fleet-accounts.js'

const STUDIO = '018f0000-0000-7000-8000-0000000000aa'
const own = { name: 'router:work', kind: 'subscription', holder: 'self', holder_name: 'Laptop', shared: true, this_machine: true, usable_here: true }
const received = { name: 'router:team', kind: 'subscription', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: true }
const piElsewhere = { name: 'pi:anthropic', kind: 'oauth', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: true }
const key = { name: 'router:key', kind: 'api_key', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: true }
const accounts = [own, received, piElsewhere, key]

describe('fleet sign-in holders', () => {
  it('reads the accounts from fleet.credentials on this computer', async () => {
    const invoke = vi.fn(async () => ({ share_models: true, accounts }))
    expect(await readFleetAccounts({ invoke })).toEqual(accounts)
    expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'fleet.credentials', body: {} })
  })

  it('answers null when core lists no accounts or refuses', async () => {
    expect(await readFleetAccounts({ invoke: async () => ({ share_models: true, secrets: [] }) })).toBeNull()
    expect(await readFleetAccounts({ invoke: async () => { throw new Error('offline') } })).toBeNull()
  })

  it('matches a router row by its id, and a received row by its holder', () => {
    expect(routerHold(accounts, 'work')).toBe(own)
    expect(routerHold(accounts, `shared:swarm:${STUDIO}:team`)).toBe(received)
    expect(routerHold(accounts, 'shared:swarm:another:team')).toBeNull()
    // A key has no sign-in to hold.
    expect(routerHold(accounts, `shared:swarm:${STUDIO}:key`)).toBeNull()
    expect(routerHold(null, 'work')).toBeNull()
  })

  it('shows a sign-in this machine keeps to itself as nothing', () => {
    expect(routerHold([{ ...own, shared: false }], 'work')).toBeNull()
  })

  it('matches a Pi sign-in by provider, this machine first', () => {
    expect(piHold(accounts, 'anthropic')).toBe(piElsewhere)
    const mine = { ...piElsewhere, holder: 'self', holder_name: 'Laptop', this_machine: true }
    expect(piHold([piElsewhere, mine], 'anthropic')).toBe(mine)
    expect(piHold(accounts, 'openai')).toBeNull()
  })

  it('states the holder in one line', () => {
    expect(holdLine(own)).toBe('Signed in here')
    expect(holdLine(received)).toBe('Signed in on Studio')
    expect(holdLine({ ...received, holder_name: '' })).toBe('Signed in on another machine')
    expect(holdLine(received, true)).toBe('Moving the sign-in here.')
  })

  it('lists the sign-ins other machines hold that this machine cannot use yet', () => {
    const arriving = { name: 'pi:openai-codex', provider: 'openai-codex', kind: 'oauth', label: 'openai-codex', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: false }
    const kept = { name: 'router:home', provider: 'anthropic', family: 'anthropic', kind: 'subscription', label: 'Home', holder: STUDIO, holder_name: 'Studio', shared: false, this_machine: false, usable_here: false }
    const keyElsewhere = { ...key, usable_here: false }
    expect(otherSignIns([...accounts, arriving, kept, keyElsewhere])).toEqual([
      { id: `${STUDIO}/pi:openai-codex`, label: 'openai-codex', provider: 'openai-codex', line: 'Signed in on Studio, arriving' },
      { id: `${STUDIO}/router:home`, label: 'Home', provider: 'anthropic', line: 'Signed in on Studio, not shared' },
    ])
    expect(otherSignIns(null)).toEqual([])
  })
})

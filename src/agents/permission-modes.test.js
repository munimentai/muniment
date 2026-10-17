import { expect, it, vi } from 'vitest'
import { agentMode, loadModes, modeIcon, modePermissions, nextMode } from './permission-modes.js'

// Core's `permission.modes` answer on a runtime with bypass off.
const listing = {
  modes: [
    { id: 'manual', title: 'Ask before changes', description: 'Reads run.', available: true },
    { id: 'edit_auto', title: 'Edit automatically', description: 'Edits run.', available: true },
    { id: 'plan', title: 'Plan first', description: 'Plans first.', available: true },
    { id: 'auto', title: 'Auto review', description: 'A model reviews.', available: true },
    { id: 'dont_ask', title: "Don't ask", description: 'Denies the rest.', available: true },
    { id: 'bypass', title: 'Bypass permissions', description: 'Runs everything.', available: false, reason: 'Bypass is off.' },
  ],
  default_thread_mode: 'manual',
  default_agent_mode: 'auto',
}

it('reads the modes from core and answers null for a runtime that lists none', async () => {
  const tauri = { invoke: vi.fn().mockResolvedValueOnce(listing).mockRejectedValueOnce(new Error('The agent action is unknown.')) }
  expect(await loadModes(tauri, 'box')).toBe(listing)
  expect(tauri.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'box', operation: 'permission.modes', body: {} })
  expect(await loadModes(tauri, null)).toBeNull()
})

it('shows an agent without a mode in the default agent mode, and its own mode as chosen', () => {
  expect(agentMode({}, listing)).toMatchObject({ id: 'auto', chosen: false })
  expect(agentMode({ permissions: { mode: 'plan' } }, listing)).toMatchObject({ id: 'plan', chosen: true })
  expect(agentMode({ permissions: { mode: 'plan' } }, null)).toBeNull()
})

it('saves the mode id only when one is chosen, and keeps the rules', () => {
  const agent = { permissions: { mode: 'manual', always_allow: ['rule'], ask_first: [], deny: ['no'] } }
  expect(modePermissions(agent, listing.modes[4])).toEqual({ mode: 'dont_ask', always_allow: ['rule'], ask_first: [], deny: ['no'] })
  expect(modePermissions(agent, null)).toEqual({ always_allow: ['rule'], ask_first: [], deny: ['no'] })
  expect(modePermissions({}, listing.modes[0])).toEqual({ mode: 'manual', always_allow: [], ask_first: [] })
})

it('cycles through the available modes and skips bypass while it is off', () => {
  expect(nextMode(listing, 'manual').id).toBe('edit_auto')
  expect(nextMode(listing, 'dont_ask').id).toBe('manual')
  expect(nextMode({ ...listing, modes: listing.modes.map((mode) => ({ ...mode, available: true })) }, 'dont_ask').id).toBe('bypass')
  expect(nextMode(null, 'manual')).toBeNull()
})

it('names the icon the agent card uses for every mode', () => {
  expect(listing.modes.map(modeIcon)).toEqual(['hand', 'pencil', 'map', 'zap', 'shield-check', 'triangle-alert'])
  expect(modeIcon('plan')).toBe('map')
})

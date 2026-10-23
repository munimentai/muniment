import { expect, it, vi } from 'vitest'
import { agentMode, loadModes, modeIcon, modePermissions, nextMode, watchModes } from './permission-modes.js'

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

it('saves the mode id only when one is chosen', () => {
  const agent = { permissions: { mode: 'manual' } }
  expect(modePermissions(agent, listing.modes[4])).toEqual({ mode: 'dont_ask' })
  expect(modePermissions(agent, null)).toEqual({})
  expect(modePermissions({}, listing.modes[0])).toEqual({ mode: 'manual' })
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

it('asks again for the modes until a fresh install\'s runtime lists them', async () => {
  vi.useFakeTimers()
  try {
    const tauri = { invoke: vi.fn()
      .mockRejectedValueOnce(new Error('Choose a Home folder first.'))
      .mockResolvedValueOnce({ modes: [] })
      .mockResolvedValueOnce(listing) }
    const onlisting = vi.fn()
    watchModes(tauri, 'local', onlisting, [10, 20, 30])
    await vi.advanceTimersByTimeAsync(0)
    expect(onlisting).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(10)
    expect(onlisting).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(20)
    expect(onlisting).toHaveBeenCalledWith(listing)
    expect(tauri.invoke).toHaveBeenCalledTimes(3)
  } finally { vi.useRealTimers() }
})

it('stops asking for the modes once the composer moves to another runtime', async () => {
  vi.useFakeTimers()
  try {
    const tauri = { invoke: vi.fn().mockResolvedValue(null) }
    const stop = watchModes(tauri, 'local', vi.fn(), [10, 10])
    await vi.advanceTimersByTimeAsync(0)
    stop()
    await vi.advanceTimersByTimeAsync(100)
    expect(tauri.invoke).toHaveBeenCalledTimes(1)
  } finally { vi.useRealTimers() }
})

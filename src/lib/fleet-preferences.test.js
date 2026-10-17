import { afterEach, describe, expect, it, vi } from 'vitest'
import { IMPORTED_MARKER, startFleetPreferences } from './fleet-preferences.js'
import { THEME_STORAGE_KEY, commitTheme, readStoredTheme } from './theme-state.js'
import { MARK_STORAGE_KEY, commitMark, readStoredMark } from './thinking-mark-state.js'
import { readSearchEngine, saveSearchEngine } from './browser-settings.js'

let stop = () => {}
afterEach(() => {
  stop()
  localStorage.clear()
  delete document.documentElement.dataset.theme
  delete document.documentElement.dataset.scheme
})

// A runtime that answers the preferences operations from `values`, or refuses
// every one of them.
function runtime(values = {}, { refuse = false } = {}) {
  const invoke = vi.fn(async (command, { operation, body }) => {
    if (refuse) throw new Error('The operation is unknown.')
    if (operation === 'preferences.set') Object.assign(values, body.values)
    if (operation === 'preferences.import') {
      for (const [key, value] of Object.entries(body.values)) {
        const name = key.replace(/^muniment\./, '')
        if (!(name in values)) values[name] = value
      }
    }
    return { values: { ...values }, fleet_keys: [], local_keys: [] }
  })
  return { invoke }
}

const operations = (tauri) => tauri.invoke.mock.calls.map(([, { operation }]) => operation)

function listener() {
  const handlers = {}
  const listen = vi.fn(async (event, handle) => { handlers[event] = handle; return () => { delete handlers[event] } })
  return { listen, emit: (event, payload) => handlers[event]?.({ payload }) }
}

describe('fleet preferences', () => {
  it('imports this device values once and only after core accepts them', async () => {
    commitMark('ear')
    localStorage.setItem('muniment.sidebar-width', '300')
    const refused = runtime({}, { refuse: true })
    stop = startFleetPreferences(refused)
    await vi.waitFor(() => expect(refused.invoke).toHaveBeenCalled())
    stop()
    expect(localStorage.getItem(IMPORTED_MARKER)).toBeNull()

    const tauri = runtime()
    stop = startFleetPreferences(tauri)
    await vi.waitFor(() => expect(operations(tauri)).toEqual(['preferences.import', 'preferences.get']))
    const [command, request] = tauri.invoke.mock.calls[0]
    expect(command).toBe('agent_request')
    expect(request).toEqual({ runtime: 'local', operation: 'preferences.import', body: { values: { 'muniment.thinking-mark': 'ear' } } })
    expect(localStorage.getItem(IMPORTED_MARKER)).toBe('1')
    stop()

    const again = runtime()
    stop = startFleetPreferences(again)
    await vi.waitFor(() => expect(operations(again)).toEqual(['preferences.get']))
  })

  it('applies the values core holds', async () => {
    const theme = JSON.stringify({ mode: 'light', palette: 'iris' })
    const tauri = runtime({ theme, 'thinking-mark': 'text', 'browser.search': 'brave', 'memory-recall': { learn: true, use: true, ask: false } })
    stop = startFleetPreferences(tauri)
    await vi.waitFor(() => expect(readStoredMark()).toBe('text'))
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe(theme)
    expect(document.documentElement.dataset.theme).toBe('iris-light')
    expect(readSearchEngine()).toBe('brave')
  })

  it('sends a local change to core under its fleet key', async () => {
    const tauri = runtime()
    stop = startFleetPreferences(tauri)
    await vi.waitFor(() => expect(operations(tauri)).toContain('preferences.get'))
    commitTheme({ mode: 'dark', palette: 'clay' })
    saveSearchEngine('duckduckgo')
    const sets = tauri.invoke.mock.calls.filter(([, { operation }]) => operation === 'preferences.set').map(([, { body }]) => body)
    expect(sets).toEqual([
      { values: { theme: JSON.stringify({ mode: 'dark', palette: 'clay' }) } },
      { values: { 'browser.search': 'duckduckgo' } },
    ])
  })

  it('applies a change core announces, and a removal restores the default', async () => {
    const tauri = runtime()
    const events = listener()
    stop = startFleetPreferences(tauri, events.listen)
    await vi.waitFor(() => expect(operations(tauri)).toContain('preferences.get'))
    events.emit('preferences-changed', { runtimeId: 'local', change: { source: 'fleet', values: { 'thinking-mark': 'still' } } })
    expect(readStoredMark()).toBe('still')
    events.emit('preferences-changed', { runtimeId: 'remote-1', change: { source: 'fleet', values: { 'thinking-mark': 'ear' } } })
    expect(readStoredMark()).toBe('still')
    events.emit('preferences-changed', { runtimeId: 'local', change: { source: 'fleet', values: { 'thinking-mark': null } } })
    expect(readStoredMark()).toBe('motion')
    expect(operations(tauri)).not.toContain('preferences.set')
  })

  it('keeps every preference in localStorage when core refuses the operations', async () => {
    commitTheme({ mode: 'light', palette: 'harbor' })
    const tauri = runtime({}, { refuse: true })
    stop = startFleetPreferences(tauri)
    await vi.waitFor(() => expect(operations(tauri)).toEqual(['preferences.import', 'preferences.get']))
    commitMark('ear')
    await Promise.resolve()
    expect(operations(tauri)).toEqual(['preferences.import', 'preferences.get'])
    expect(readStoredMark()).toBe('ear')
    expect(readStoredTheme()).toEqual({ mode: 'light', palette: 'harbor' })
  })
})

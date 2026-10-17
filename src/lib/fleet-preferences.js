// The fleet-wide preferences live in core's `preferences.*` operations on this
// computer's runtime, so every machine in the swarm shows the same theme, type,
// thinking mark, agent avatars and browser settings. localStorage keeps a copy
// for the first paint, and stays the only store when the runtime refuses the
// operations. Machine-local preferences stay in localStorage alone. Core's
// `memory-recall` key is the memory recall settings, which Memory reads itself.
import { agentRequest } from '../agents/agent-runtime.js'
import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'
import { THEME_EVENT, THEME_STORAGE_KEY, commitTheme } from './theme-state.js'
import { TYPE_EVENT, TYPE_STORAGE_KEY, commitType } from './type-state.js'
import { MARK_EVENT, MARK_STORAGE_KEY, commitMark } from './thinking-mark-state.js'
import { AVATAR_STYLE_EVENT, AVATAR_STYLE_STORAGE_KEY, commitAvatarStyle } from './avatar-style-state.js'
import { DEFAULT_SEARCH, HOMEPAGE_EVENT, HOMEPAGE_STORAGE_KEY, SEARCH_EVENT, SEARCH_STORAGE_KEY, saveHomepage, saveSearchEngine } from './browser-settings.js'

// Set once this device's stored values reach core, so the move runs once.
export const IMPORTED_MARKER = 'muniment.fleet-preferences-imported'

// Each fleet-wide core key: its localStorage key, the window event its setter
// sends, and how a value from core applies. Core holds the stored text as it
// is, and a null value restores the default.
export const FLEET_PREFERENCES = {
  theme: { storage: THEME_STORAGE_KEY, event: THEME_EVENT, apply: (value) => commitTheme(value) },
  type: { storage: TYPE_STORAGE_KEY, event: TYPE_EVENT, apply: (value) => commitType(value) },
  'thinking-mark': { storage: MARK_STORAGE_KEY, event: MARK_EVENT, apply: commitMark },
  'agent-avatars': { storage: AVATAR_STYLE_STORAGE_KEY, event: AVATAR_STYLE_EVENT, apply: commitAvatarStyle },
  'browser.search': { storage: SEARCH_STORAGE_KEY, event: SEARCH_EVENT, apply: (value) => saveSearchEngine(value ?? DEFAULT_SEARCH) },
  'browser.homepage': { storage: HOMEPAGE_STORAGE_KEY, event: HOMEPAGE_EVENT, apply: (value) => saveHomepage(value ?? '') },
}

const stored = (key) => { try { return localStorage.getItem(key) } catch (_) { return null } }

// The fleet-wide values this device holds, under their localStorage keys.
export function storedFleetValues() {
  const values = {}
  for (const { storage } of Object.values(FLEET_PREFERENCES)) {
    const value = stored(storage)
    if (value !== null) values[storage] = value
  }
  return values
}

// Moves this device's values to core once, reads core's values, sends each
// local change to core and applies each change core announces. A runtime that
// refuses the operations leaves every preference in localStorage. Answers the
// function that stops it.
export function startFleetPreferences(tauri, listen) {
  if (!tauri) return () => {}
  const request = (operation, body) => agentRequest(tauri, LOCAL_RUNTIME_ID, operation, body)
  const stops = []
  let available = false
  let applying = false
  let stopped = false

  function apply(values) {
    applying = true
    try {
      for (const [key, value] of Object.entries(values ?? {})) {
        const preference = FLEET_PREFERENCES[key]
        if (!preference || (value !== null && value === stored(preference.storage))) continue
        try { preference.apply(value) } catch (_) {}
      }
    } finally {
      applying = false
    }
  }

  for (const [key, preference] of Object.entries(FLEET_PREFERENCES)) {
    const send = () => {
      if (!available || applying) return
      const value = stored(preference.storage)
      if (value !== null) void request('preferences.set', { values: { [key]: value } }).catch(() => {})
    }
    window.addEventListener(preference.event, send)
    stops.push(() => window.removeEventListener(preference.event, send))
  }

  void Promise.resolve(listen?.('preferences-changed', ({ payload }) => {
    if ((payload?.runtimeId || LOCAL_RUNTIME_ID) === LOCAL_RUNTIME_ID) apply(payload?.change?.values)
  })).then((unlisten) => { if (stopped) unlisten?.(); else stops.push(unlisten) }).catch(() => {})

  void (async () => {
    // A refused import leaves the marker unset, so the next start tries it
    // again, and core's values still apply now.
    if (stored(IMPORTED_MARKER) === null) {
      try {
        await request('preferences.import', { values: storedFleetValues() })
        try { localStorage.setItem(IMPORTED_MARKER, '1') } catch (_) {}
      } catch (_) {}
    }
    const answer = await request('preferences.get', {})
    if (stopped) return
    available = true
    apply(answer?.values)
  })().catch(() => {})

  return () => {
    stopped = true
    for (const stop of stops) stop?.()
  }
}

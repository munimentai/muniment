// The permission modes, as core's `permission.modes` lists them on one
// runtime: `{ modes: [{ id, title, description, available, reason? }],
// default_thread_mode, default_agent_mode }`. Core owns the list, the
// titles, the descriptions and why a mode is unavailable. The desktop only
// picks an icon for each id.
import { agentRequest } from './agent-runtime.js'

const ICONS = { manual: 'hand', edit_auto: 'pencil', plan: 'map', auto: 'zap', dont_ask: 'shield-check', bypass: 'triangle-alert' }

export function modeIcon(mode) {
  return ICONS[typeof mode === 'string' ? mode : mode?.id] ?? 'shield-check'
}

// The runtime's mode listing, or null when it answers none, such as an
// older runtime.
export async function loadModes(tauri, runtime) {
  try {
    const listing = await agentRequest(tauri, runtime, 'permission.modes', {})
    return Array.isArray(listing?.modes) && listing.modes.length ? listing : null
  } catch {
    return null
  }
}

export function findMode(listing, id) {
  return listing?.modes?.find((mode) => mode.id === id) ?? null
}

// The mode an agent runs in: its saved `permissions.mode`, else the
// runtime's default agent mode. `chosen` is false while the agent follows
// the default.
export function agentMode(agent, listing) {
  const saved = agent?.permissions?.mode
  const mode = findMode(listing, saved ?? listing?.default_agent_mode)
  return mode ? { ...mode, chosen: !!saved } : null
}

// The permissions an agent saves to enter a mode, or to follow the default
// again with `null`. Its rules stay as they are.
export function modePermissions(agent, mode) {
  const { mode: _saved, ...rules } = agent?.permissions ?? { always_allow: [], ask_first: [] }
  return mode ? { ...rules, mode: mode.id } : rules
}

// The next mode Shift+Tab moves to: the next available mode after `current`,
// wrapping round.
export function nextMode(listing, current) {
  const modes = listing?.modes ?? []
  const start = modes.findIndex((mode) => mode.id === current)
  for (let step = 1; step <= modes.length; step += 1) {
    const mode = modes[(start + step + modes.length) % modes.length]
    if (mode.available !== false) return mode
  }
  return null
}

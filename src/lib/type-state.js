// The type preference of this device: one size step for the body register,
// and a picked family for the human and the mono register. The step scales
// every `--text-*` token in src/styles/tokens.css together, and a pick sits in
// front of the shipped family in `--font-human` or `--font-mono`. Nothing here
// leaves the device; the value lives beside `muniment.theme`.
import { shortcutDisplayLabel } from './artifact-rail-state.js'

export const TYPE_STORAGE_KEY = 'muniment.type'
export const TYPE_EVENT = 'muniment:type'
export const SIZE_STEP_MIN = -2
export const SIZE_STEP_MAX = 4
// One step moves the register by a tenth of its shipped size.
export const SIZE_STEP_RATIO = 0.1
export const BASE_TEXT_TOKENS = {
  '--text-12': 12,
  '--text-13': 13,
  '--text-15': 15,
  '--text-17': 17,
  '--text-22': 22,
  '--text-28': 28,
  '--text-provenance': 11.5,
}
export const BODY_TOKEN = '--text-15'
export const FONT_TOKENS = { human: '--font-human', mono: '--font-mono', heading: '--font-heading' }
export const SHIPPED_FONTS = { human: 'Schibsted Grotesk', mono: 'Commit Mono', heading: 'Schibsted Grotesk' }
const SHIPPED_STACKS = { heading: "'Schibsted Grotesk', system-ui, sans-serif", human: "'Schibsted Grotesk', system-ui, sans-serif", mono: "'Commit Mono', ui-monospace, monospace" }
const MAX_FAMILY_LENGTH = 120

const defaults = () => ({ step: 0, human: null, mono: null, heading: null })

export function validFamily(name) {
  return typeof name === 'string'
    && name.trim().length > 0
    && name.length <= MAX_FAMILY_LENGTH
    && !/[\x00-\x1f\x7f'"\\;{}]/.test(name)
}

// Missing, malformed, or out-of-range values restore the shipped defaults.
export function parseType(stored) {
  const state = defaults()
  if (stored && typeof stored === 'object') return parseType(JSON.stringify(stored))
  if (typeof stored !== 'string' || !stored) return state
  let parsed
  try { parsed = JSON.parse(stored) } catch (_) { return state }
  if (!parsed || typeof parsed !== 'object') return state
  if (Number.isInteger(parsed.step) && parsed.step >= SIZE_STEP_MIN && parsed.step <= SIZE_STEP_MAX) state.step = parsed.step
  if (validFamily(parsed.human)) state.human = parsed.human.trim()
  if (validFamily(parsed.mono)) state.mono = parsed.mono.trim()
  if (validFamily(parsed.heading)) state.heading = parsed.heading.trim()
  return state
}

export function serializeType(type) {
  const { step, human, mono, heading } = parseType(type)
  return JSON.stringify({ step, human, mono, heading })
}

export function sizeScale(step) {
  return 1 + step * SIZE_STEP_RATIO
}

const roundPx = (value) => Math.round(value * 10) / 10

// The body size the step yields, in pixels.
export function bodySize(step) {
  return roundPx(BASE_TEXT_TOKENS[BODY_TOKEN] * sizeScale(step))
}

export function stepType(type, delta) {
  const step = Math.min(SIZE_STEP_MAX, Math.max(SIZE_STEP_MIN, type.step + delta))
  return { ...type, step }
}

export function fontStack(register, family) {
  return family ? `'${family}', ${SHIPPED_STACKS[register]}` : null
}

// The root carries the scaled register and the picked families as inline
// custom properties. The shipped values need nothing, so they clear them.
export function applyType(root, type) {
  const scale = sizeScale(type.step)
  for (const [token, base] of Object.entries(BASE_TEXT_TOKENS)) {
    if (type.step === 0) root.style.removeProperty(token)
    else root.style.setProperty(token, `${roundPx(base * scale)}px`)
  }
  for (const [register, token] of Object.entries(FONT_TOKENS)) {
    const stack = fontStack(register, type[register])
    if (stack) root.style.setProperty(token, stack)
    else root.style.removeProperty(token)
  }
}

export function readStoredType() {
  try {
    return parseType(localStorage.getItem(TYPE_STORAGE_KEY))
  } catch (_) {
    return parseType(null)
  }
}

// Applies, stores and announces one change, so Preferences and the shortcut
// handler stay in step wherever the change starts.
export function commitType(type, root = document.documentElement) {
  const next = parseType(serializeType(type))
  applyType(root, next)
  try { localStorage.setItem(TYPE_STORAGE_KEY, serializeType(next)) } catch (_) {}
  try { window.dispatchEvent(new CustomEvent(TYPE_EVENT, { detail: next })) } catch (_) {}
  return next
}

// The super key with `=` or `+` raises the size, with `-` lowers it, and with
// `0` returns the default. Command on macOS, Control elsewhere.
export function typeSizeShortcutStep(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  const superKey = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey
  if (!superKey || event.altKey) return null
  if (event.key === '=' || event.key === '+') return 1
  if (event.key === '-' || event.key === '_') return -1
  if (event.key === '0' && !event.shiftKey) return 0
  return null
}

export function typeSizeShortcut(kind, platform = navigator.platform) {
  const modifier = platform.startsWith('Mac') ? 'Meta' : 'Control'
  return `${modifier}+${{ larger: '=', smaller: '-', default: '0' }[kind]}`
}

export function typeSizeShortcutLabel(kind, platform = navigator.platform) {
  return shortcutDisplayLabel(typeSizeShortcut(kind, platform))
}

// The installed families that contain the query, in order, capped so the list
// stays a list. An empty query shows the first families.
export function filterFonts(fonts, query, limit = 8) {
  const needle = (query ?? '').trim().toLowerCase()
  const seen = new Set()
  const matches = []
  for (const name of fonts ?? []) {
    if (!validFamily(name)) continue
    const key = name.toLowerCase()
    if (seen.has(key) || (needle && !key.includes(needle))) continue
    seen.add(key)
    matches.push(name)
    if (matches.length >= limit) break
  }
  return matches
}

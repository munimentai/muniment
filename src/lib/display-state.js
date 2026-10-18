// The display preference of this device, beside the type preference: the
// interface zoom, the density and the motion choice. Zoom scales the whole
// window, controls and spacing included, through the webview. The text size in
// type-state.js scales only the `--text-*` register. Density changes spacing
// only. Motion follows the OS, or the pick overrides it. The root carries the
// density and the motion that applies as data-density and data-motion, and
// every reduced motion rule reads data-motion, so one switch covers all of them.
export const DISPLAY_STORAGE_KEY = 'muniment.display'
export const DISPLAY_EVENT = 'muniment:display'
export const ZOOM_OPTIONS = [0.9, 1, 1.1, 1.25, 1.5]
export const DENSITY_OPTIONS = [['comfortable', 'Comfortable'], ['compact', 'Compact']]
export const MOTION_OPTIONS = [['system', 'System'], ['reduce', 'Reduce'], ['full', 'Full']]
const REDUCED_QUERY = '(prefers-reduced-motion: reduce)'

const defaults = () => ({ zoom: 1, density: 'comfortable', motion: 'system' })

// Missing, malformed or unknown values restore the defaults.
export function parseDisplay(stored) {
  const state = defaults()
  let parsed = stored
  if (typeof stored === 'string') {
    try { parsed = JSON.parse(stored) } catch (_) { return state }
  }
  if (!parsed || typeof parsed !== 'object') return state
  if (ZOOM_OPTIONS.includes(parsed.zoom)) state.zoom = parsed.zoom
  if (DENSITY_OPTIONS.some(([id]) => id === parsed.density)) state.density = parsed.density
  if (MOTION_OPTIONS.some(([id]) => id === parsed.motion)) state.motion = parsed.motion
  return state
}

export const zoomLabel = (zoom) => `${Math.round(zoom * 100)}%`

export function readStoredDisplay() {
  try {
    return parseDisplay(localStorage.getItem(DISPLAY_STORAGE_KEY))
  } catch (_) {
    return defaults()
  }
}

const osReduces = () => {
  try { return !!globalThis.matchMedia?.(REDUCED_QUERY)?.matches } catch (_) { return false }
}

// Whether motion is reduced now: the pick, or the OS while the pick is System.
export function motionReduced(display = readStoredDisplay()) {
  return display.motion === 'reduce' || (display.motion === 'system' && osReduces())
}

// The webview zoom of the main window. A rect the page measures is in CSS
// pixels, and the window places native views and menus in logical pixels, so
// those callers scale by it.
let appliedZoom = 1
export const currentZoom = () => appliedZoom
export const toWindowPixels = (value) => value * appliedZoom

export function applyDisplay(root, display, webview = globalThis.__TAURI__?.webview?.getCurrentWebview?.()) {
  const next = parseDisplay(display)
  if (next.density === 'compact') root.dataset.density = 'compact'
  else delete root.dataset.density
  if (motionReduced(next)) root.dataset.motion = 'reduce'
  else delete root.dataset.motion
  if (webview?.setZoom && next.zoom !== appliedZoom) {
    appliedZoom = next.zoom
    Promise.resolve(webview.setZoom(next.zoom)).catch(() => {})
  }
  return next
}

// An OS change of the reduced motion setting applies at once while the pick is System.
export function followSystemMotion(root) {
  try {
    globalThis.matchMedia?.(REDUCED_QUERY)?.addEventListener?.('change', () => applyDisplay(root, readStoredDisplay()))
  } catch (_) {}
}

// Calls `change` whenever reduced motion may have changed: an OS change or a
// new pick. Answers the function that stops it.
export function watchMotion(change) {
  let media = null
  try { media = globalThis.matchMedia?.(REDUCED_QUERY) ?? null } catch (_) {}
  media?.addEventListener?.('change', change)
  globalThis.addEventListener?.(DISPLAY_EVENT, change)
  return () => { media?.removeEventListener?.('change', change); globalThis.removeEventListener?.(DISPLAY_EVENT, change) }
}

// Applies, stores and announces one change.
export function commitDisplay(display, root = document.documentElement) {
  const next = applyDisplay(root, display)
  try { localStorage.setItem(DISPLAY_STORAGE_KEY, JSON.stringify(next)) } catch (_) {}
  try { window.dispatchEvent(new CustomEvent(DISPLAY_EVENT, { detail: next })) } catch (_) {}
  return next
}

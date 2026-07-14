import milledRingSvg from '../../src-tauri/icons/muniment-milled-ring.svg?raw'

// Keep every in-app mark on the same owner-supplied geometry used to generate
// the native application icons.
const pathMatch = milledRingSvg.match(/<path d="([^"]+)"/)

export const MILLED_RING_PATH = pathMatch[1]

export function ringPath() {
  return MILLED_RING_PATH
}

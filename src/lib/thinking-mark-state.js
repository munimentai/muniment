export const MARK_STORAGE_KEY = 'muniment.thinking-mark'
export const MARK_EVENT = 'muniment:thinking-mark'

// The thinking indicator: the elephant that moves with the run, the elephant
// that only flaps its ear, the still elephant, or the stage word alone with a
// text sheen. Reduced motion stills every one of them.
export const MARK_OPTIONS = [
  ['motion', 'Motion elephant'],
  ['ear', 'Ear flap'],
  ['still', 'Still elephant'],
  ['text', 'Text sheen'],
]
export const DEFAULT_MARK = 'motion'

export function parseMark(stored) {
  return MARK_OPTIONS.some(([id]) => id === stored) ? stored : DEFAULT_MARK
}

export function readStoredMark() {
  try {
    return parseMark(localStorage.getItem(MARK_STORAGE_KEY))
  } catch (_) {
    return DEFAULT_MARK
  }
}

// Stores and announces one change, so every mark on screen follows it.
export function commitMark(mark) {
  const next = parseMark(mark)
  try { localStorage.setItem(MARK_STORAGE_KEY, next) } catch (_) {}
  try { window.dispatchEvent(new CustomEvent(MARK_EVENT, { detail: next })) } catch (_) {}
  return next
}

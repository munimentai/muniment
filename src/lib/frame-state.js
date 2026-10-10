export const FRAME_STORAGE_KEY = 'muniment.frame'

// The frame: Framed sets the sidebar on the paper frame and makes each content
// panel a card. Frameless drops the frame and divides flat panels with one-pixel
// seams. Both work with every color theme. The root carries the pick as
// data-frame, and src/styles/panels.css reads it.
export const FRAME_OPTIONS = [
  ['framed', 'Framed'],
  ['frameless', 'Frameless'],
]
export const DEFAULT_FRAME = 'framed'

export function parseFrame(stored) {
  return FRAME_OPTIONS.some(([id]) => id === stored) ? stored : DEFAULT_FRAME
}

export function readStoredFrame() {
  try {
    return parseFrame(localStorage.getItem(FRAME_STORAGE_KEY))
  } catch (_) {
    return DEFAULT_FRAME
  }
}

export function applyFrame(root, frame) {
  root.dataset.frame = parseFrame(frame)
}

// Stores and applies one change, so the workspace follows it at once.
export function commitFrame(frame) {
  const next = parseFrame(frame)
  try { localStorage.setItem(FRAME_STORAGE_KEY, next) } catch (_) {}
  applyFrame(document.documentElement, next)
  return next
}

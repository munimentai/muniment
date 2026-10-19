// Which input leads: the root carries data-pointer from a pointer press until
// a key moves focus. base.css draws no focus ring on a control while the
// pointer leads, since the pointer shows where it is, so a menu or dialog that
// focuses its first control on a click shows no ring there.
export const FOCUS_KEYS = ['Tab', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown']

export function trackFocusSource(root, target = window) {
  const pointer = () => { root.dataset.pointer = '' }
  const key = (event) => { if (FOCUS_KEYS.includes(event.key)) delete root.dataset.pointer }
  target.addEventListener('pointerdown', pointer, true)
  target.addEventListener('keydown', key, true)
  return () => {
    target.removeEventListener('pointerdown', pointer, true)
    target.removeEventListener('keydown', key, true)
  }
}

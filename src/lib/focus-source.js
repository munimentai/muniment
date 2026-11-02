// Which input leads: the root carries data-pointer from a pointer press until
// a key moves focus or presses a control. base.css draws no focus ring on a
// control while the pointer leads, since the pointer shows where it is, so a
// menu or dialog that focuses its first control on a click shows no ring there.
export const FOCUS_KEYS = ['Tab', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown']
// Enter, Space and Escape press or dismiss a control, so the keys lead after
// them too. In a text field they type or send, and the pointer keeps the lead.
const PRESS_KEYS = ['Enter', ' ', 'Escape']
const typing = (target) => !!target?.closest?.('input, textarea, select, [contenteditable]:not([contenteditable="false"])')

export function trackFocusSource(root, target = window) {
  const pointer = () => { root.dataset.pointer = '' }
  const key = (event) => {
    if (FOCUS_KEYS.includes(event.key) || (PRESS_KEYS.includes(event.key) && !typing(event.target))) delete root.dataset.pointer
  }
  target.addEventListener('pointerdown', pointer, true)
  target.addEventListener('keydown', key, true)
  return () => {
    target.removeEventListener('pointerdown', pointer, true)
    target.removeEventListener('keydown', key, true)
  }
}

// Code that moves focus, such as a closing confirmation returning it to its
// opener, shows the ring when the keys lead, as Tab would. A focus() from
// code after the old focus left the page matches no :focus-visible in some
// engines, so data-key-focus draws the same ring until the control blurs.
export function focusLikeKeys(node, options = {}) {
  if (!node?.isConnected || typeof node.focus !== 'function') return false
  const keys = !document.documentElement.hasAttribute('data-pointer')
  node.focus({ ...options, focusVisible: keys })
  if (document.activeElement !== node) return false
  if (keys && !matchesFocusVisible(node)) {
    node.setAttribute('data-key-focus', '')
    node.addEventListener('blur', () => node.removeAttribute('data-key-focus'), { once: true })
  }
  return true
}

function matchesFocusVisible(node) {
  try { return node.matches(':focus-visible') } catch (_) { return false }
}

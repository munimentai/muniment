// Keyboard focus that a page change or a closed panel leaves on a hidden or
// removed node still draws the system's input-source mark at its old caret,
// and typed keys reach no field the user sees. These helpers move it to the
// page the user now sees.

// Whether the user sees the element: it is on the page, nothing hides it,
// and no ancestor is inert or out of the layout.
export function shown(element) {
  if (!element?.isConnected || element === document.body || element === document.documentElement) return false
  if (element.closest('[hidden], [inert]')) return false
  if (typeof element.checkVisibility === 'function' && !element.checkVisibility({ visibilityProperty: true })) return false
  if (getComputedStyle(element).visibility === 'hidden') return false
  for (let node = element; node; node = node.parentElement) {
    if (getComputedStyle(node).display === 'none') return false
  }
  return true
}

// Leaves focus where the user sees it. When it sits on a hidden or removed
// node, it moves to the first heading the page shows outside `skip`, else to
// `fallback`, such as the composer, else it leaves the node. Returns the
// element that took focus, or null.
export function landFocus(root, { skip = '', fallback = null } = {}) {
  const active = document.activeElement
  // Focus on the body sits on no field, so nothing draws a caret mark.
  if (!active || active === document.body || shown(active)) return null
  const heading = [...(root?.querySelectorAll('h1, h2, h3') ?? [])]
    .find((node) => !(skip && node.closest(skip)) && shown(node))
  const target = heading ?? (shown(fallback) ? fallback : null)
  if (target) {
    // A heading takes focus from code only, never from Tab.
    if (target === heading && !heading.hasAttribute('tabindex')) heading.setAttribute('tabindex', '-1')
    target.focus({ preventScroll: true })
    if (document.activeElement === target) return target
  }
  active.blur?.()
  return null
}

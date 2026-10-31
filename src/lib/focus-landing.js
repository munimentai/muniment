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
// `fallback`, such as the composer, else it leaves the node. On a `page` that
// replaced the thread, focus on the body lands too, and a page with no heading
// lands on its labeled region: WebKit drops focus to the body when a click
// hides the composer, and the system's input-source mark stays at the old
// caret until a visible element takes focus. Returns the element that took
// focus, or null.
export function landFocus(root, { skip = '', fallback = null, page = false } = {}) {
  const active = document.activeElement
  const onBody = !active || active === document.body
  if (onBody ? !page : shown(active)) return null
  const first = (selector) => [...(root?.querySelectorAll(selector) ?? [])]
    .find((node) => !(skip && node.closest(skip)) && shown(node))
  const landing = first('h1, h2, h3') ?? (page ? first('section[aria-label]') : null)
  const target = landing ?? (shown(fallback) ? fallback : null)
  if (target) {
    // A heading or a region takes focus from code only, never from Tab.
    if (target === landing && !landing.hasAttribute('tabindex')) landing.setAttribute('tabindex', '-1')
    target.focus({ preventScroll: true })
    if (document.activeElement === target) return target
  }
  active?.blur?.()
  return null
}

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

const NO_TEXT = new Set(['button', 'checkbox', 'color', 'file', 'hidden', 'image', 'radio', 'range', 'reset', 'submit'])

// Whether the element takes typed text, so the system draws its caret and its
// input-source mark there. WebKit leaves that mark at the old caret when focus
// moves to an element that takes no text, and only the shell can hide it.
export function takesText(element) {
  if (!element || element === document.body) return false
  if (element.isContentEditable || element.localName === 'textarea') return true
  return element.localName === 'input' && !NO_TEXT.has(element.type)
}

// The terminal draws its prompt where AppKit draws the input-source mark,
// beside the caret of xterm's hidden text field, so the mark covers the
// prompt. AppKit shows the mark up to two seconds after the caret moves, and
// the shell hides only a mark it sees. So each focus, keystroke or output
// asks the shell to watch: it checks every 50 ms and hides the mark until
// three seconds after the last request. A burst of pokes sends at most one
// request per `throttle` ms, and the last poke of a burst sends one at the
// burst's end. `stop` ends the watch when the terminal loses focus or closes.
export const INPUT_MARK_THROTTLE_MS = 250
export function inputMarkSettler(invoke, throttle = INPUT_MARK_THROTTLE_MS) {
  let timer
  let pending = false
  const send = (command) => Promise.resolve(invoke?.(command)).catch(() => {})
  const open = () => {
    send('input_mark_watch')
    timer = setTimeout(() => {
      timer = undefined
      if (pending) { pending = false; open() }
    }, throttle)
  }
  return {
    poke() {
      if (timer === undefined) open()
      else pending = true
    },
    stop() {
      clearTimeout(timer)
      timer = undefined
      pending = false
      send('input_mark_unwatch')
    },
  }
}

// Focus that moves from one text field to another, such as from the composer
// to the search field of a picker or a popover, leaves the mark at the old
// caret too: WebKit tells AppKit of no move it sees. So each focus on a text
// field other than the last one asks `settler` to watch, and the shell hides
// the stale mark. Focus that returns to the same field asks nothing. Answers
// the function that stops listening.
export function settleTextFocusMoves(target, settler) {
  let last = null
  const moved = (event) => {
    const field = event.target
    if (!takesText(field)) return
    const from = takesText(event.relatedTarget) ? event.relatedTarget : last
    last = field
    if (from && from !== field) settler.poke()
  }
  target.addEventListener('focusin', moved)
  return () => target.removeEventListener('focusin', moved)
}

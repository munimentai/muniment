// The element a secondary press began on: the right button, or Control with
// the primary one. The context menu event that follows hit-tests again, so a
// list that re-renders between the two can put another row under the
// pointer. A row's menu opens only for the row the press began on.
const PRESS_MS = 1000
let press = null

export function recordPress(event) {
  press = event.button === 2 || (event.button === 0 && event.ctrlKey) ? { target: event.target, time: event.timeStamp } : null
}

// A secondary press on a row of the sidebar, the model picker, a menu or a
// list of choices keeps its default off: WebKit selects the word under the
// pointer and focuses the row, which scrolls the list. The context menu
// still follows. A text field keeps its own menu.
const HELD = '[data-panel="sidebar"], [data-panel="model-picker"], [role="menu"], [role="menubar"], [role="listbox"], [role="tree"]'
export function holdSecondaryPress(event) {
  if (!(event.button === 2 || (event.button === 0 && event.ctrlKey))) return
  const target = event.target instanceof Element ? event.target : event.target?.parentElement
  if (!target?.closest(HELD) || target.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')) return
  event.preventDefault()
}

// The same rows never show the browser's own menu (Reload, AutoFill). A row
// with a menu of its own still opens it, and a text field keeps the browser's.
export function holdNativeMenu(event) {
  const target = event.target instanceof Element ? event.target : event.target?.parentElement
  if (!target?.closest(HELD) || target.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')) return
  event.preventDefault()
}

if (typeof document !== 'undefined') {
  document.addEventListener('pointerdown', recordPress, true)
  document.addEventListener('mousedown', holdSecondaryPress, true)
  document.addEventListener('contextmenu', holdNativeMenu, true)
}

// Whether `event`, a pointer's context menu on `row`, follows a press that
// began outside `row`. A keyboard context menu names no point and passes.
export function pressedElsewhere(event, row = event.currentTarget) {
  const last = press
  press = null
  if (event.type !== 'contextmenu' || (!event.clientX && !event.clientY) || !last || !row) return false
  if (event.timeStamp - last.time > PRESS_MS) return false
  return !row.contains(last.target)
}

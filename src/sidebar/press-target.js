// The element a secondary press began on: the right button, or Control with
// the primary one. The context menu event that follows hit-tests again, so a
// list that re-renders between the two can put another row under the
// pointer. A row's menu opens only for the row the press began on.
const PRESS_MS = 1000
let press = null

export function recordPress(event) {
  press = event.button === 2 || (event.button === 0 && event.ctrlKey) ? { target: event.target, time: event.timeStamp } : null
}

if (typeof document !== 'undefined') document.addEventListener('pointerdown', recordPress, true)

// Whether `event`, a pointer's context menu on `row`, follows a press that
// began outside `row`. A keyboard context menu names no point and passes.
export function pressedElsewhere(event, row = event.currentTarget) {
  const last = press
  press = null
  if (event.type !== 'contextmenu' || (!event.clientX && !event.clientY) || !last || !row) return false
  if (event.timeStamp - last.time > PRESS_MS) return false
  return !row.contains(last.target)
}

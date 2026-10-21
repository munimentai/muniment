// The ring a thread row shows for core's status. A status still in progress
// travels: green while the thread works, amber while it needs the user. A
// result lights every dash at once for RESULT_MS, green for done and red for
// a failure, and then the row shows its own icon again. A read thread shows
// no ring.
export const RESULT_MS = 4000
const TRAVEL = { working: 'success', needs_you: 'warning' }
const RESULT = { done: 'success', failed: 'danger' }

export const travelTone = (status) => TRAVEL[status] ?? null
export const resultTone = (status) => RESULT[status] ?? null

// When each result first showed, by its key, so a row drawn again, such as a
// project opened again, keeps the time it had left and never lights twice.
const SHOWN_LIMIT = 500
const shown = new Map()

// The milliseconds the result under `key` stays lit.
export function resultLeft(key, now = Date.now()) {
  if (!shown.has(key)) {
    shown.set(key, now)
    if (shown.size > SHOWN_LIMIT) shown.delete(shown.keys().next().value)
  }
  return Math.max(0, RESULT_MS - (now - shown.get(key)))
}

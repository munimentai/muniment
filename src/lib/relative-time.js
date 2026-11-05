import { createSubscriber } from 'svelte/reactivity'

// One clock every relative time shares. A view that reads `minuteClock()`
// redraws each tick while it shows, so `now` turns into `1m` with no reload.
// The timer runs only while a view reads the clock.
export const CLOCK_TICK_MS = 15_000
const subscribe = createSubscriber((update) => {
  const timer = setInterval(update, CLOCK_TICK_MS)
  return () => clearInterval(timer)
})

export function minuteClock() {
  subscribe()
  return Date.now()
}

// How long ago `timestamp` was, in whole minutes, hours or days, each
// rounded down, so every relative label names the same span: a run 2.55 hours
// ago is 2h on the sidebar and on its agent's card. Null for no time.
export function elapsedSince(timestamp, now = Date.now()) {
  const at = timestamp == null ? NaN : new Date(timestamp).getTime()
  if (Number.isNaN(at)) return null
  const elapsed = Math.max(0, new Date(now).getTime() - at)
  if (elapsed < 60_000) return { unit: 'now', count: 0 }
  if (elapsed < 3_600_000) return { unit: 'm', count: Math.floor(elapsed / 60_000) }
  if (elapsed < 86_400_000) return { unit: 'h', count: Math.floor(elapsed / 3_600_000) }
  return { unit: 'd', count: Math.floor(elapsed / 86_400_000) }
}

export function relativeTime(timestamp, now = new Date()) {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return ''
  const current = new Date(now)
  if (date.getFullYear() !== current.getFullYear()) {
    return date.toLocaleDateString('en-US', { day: 'numeric', month: 'short', year: 'numeric' })
  }
  const { unit, count } = elapsedSince(date, current)
  if (unit === 'now') return 'now'
  if (unit !== 'd' || count < 7) return `${count}${unit}`
  return date.toLocaleDateString('en-US', { day: 'numeric', month: 'short' })
}

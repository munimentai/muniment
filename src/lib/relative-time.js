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

export function relativeTime(timestamp, now = new Date()) {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return ''
  const current = new Date(now)
  if (date.getFullYear() !== current.getFullYear()) {
    return date.toLocaleDateString('en-US', { day: 'numeric', month: 'short', year: 'numeric' })
  }
  const elapsed = Math.max(0, current.getTime() - date.getTime())

  if (elapsed < 60_000) return 'now'
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m`
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}h`
  if (elapsed < 604_800_000) return `${Math.floor(elapsed / 86_400_000)}d`
  return date.toLocaleDateString('en-US', { day: 'numeric', month: 'short' })
}

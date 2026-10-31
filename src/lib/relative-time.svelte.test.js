import { flushSync } from 'svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { CLOCK_TICK_MS, minuteClock, relativeTime } from './relative-time.js'

describe('the shared relative time clock', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-10-31T12:00:00Z'))
  })
  afterEach(() => vi.useRealTimers())

  it('redraws a label each tick with no reload, and stops when no view reads it', () => {
    const labels = []
    const stop = $effect.root(() => {
      $effect(() => { labels.push(relativeTime('2026-10-31T12:00:00Z', minuteClock())) })
    })
    flushSync()
    expect(labels).toEqual(['now'])
    for (let minute = 0; minute < 3; minute += 1) {
      vi.advanceTimersByTime(60_000)
      flushSync()
    }
    expect([...new Set(labels)]).toEqual(['now', '1m', '2m', '3m'])
    stop()
    const seen = labels.length
    vi.advanceTimersByTime(10 * CLOCK_TICK_MS)
    flushSync()
    expect(labels).toHaveLength(seen)
    expect(vi.getTimerCount()).toBe(0)
  })
})

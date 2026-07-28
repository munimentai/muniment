import { describe, expect, it } from 'vitest'

import { relativeTime } from './relative-time.js'

describe('relative time', () => {
  const now = new Date('2026-07-28T12:00:00Z')

  it.each([
    ['2026-07-28T11:59:01Z', 'Just now'],
    ['2026-07-28T11:59:00Z', '1m'],
    ['2026-07-28T11:00:00Z', '1h'],
    ['2026-07-27T12:00:00Z', '1d'],
    ['2026-07-21T12:00:01Z', '6d'],
    ['2026-07-21T12:00:00Z', 'Jul 21'],
    ['2026-07-28T12:01:00Z', 'Just now'],
    ['not-a-date', ''],
  ])('formats %s as %s', (timestamp, expected) => {
    expect(relativeTime(timestamp, now)).toBe(expected)
  })
})

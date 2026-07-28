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

  it('distinguishes matching dates across calendar years', () => {
    const labels = [
      relativeTime('2026-03-28T12:00:00Z', now),
      relativeTime('2025-03-28T12:00:00Z', now),
      relativeTime('2024-03-28T12:00:00Z', now),
    ]

    expect(labels).toEqual(['Mar 28', 'Mar 28, 2025', 'Mar 28, 2024'])
    expect(new Set(labels)).toHaveLength(3)
  })
})

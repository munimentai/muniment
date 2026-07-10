import { describe, expect, it } from 'vitest'

import { ringPath } from './mark.js'

describe('ringPath', () => {
  it('returns a closed SVG path with one segment per sample', () => {
    const path = ringPath(44)
    const coordinates = path.match(/\d+\.\d{2},\d+\.\d{2}/g)

    expect(path).toMatch(/^M\d+\.\d{2},\d+\.\d{2}( L\d+\.\d{2},\d+\.\d{2})+ Z$/)
    expect(coordinates).toHaveLength(45)
    expect(coordinates.at(-1)).toBe(coordinates[0])
  })

  it('produces deterministic reference geometry', () => {
    expect(ringPath(4)).toBe(
      'M40.50,24.00 L24.00,40.50 L7.50,24.00 L24.00,7.50 L40.50,24.00 Z',
    )
  })
})

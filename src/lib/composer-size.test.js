import { describe, expect, it } from 'vitest'

import { COMPOSER_MAX_ROWS, COMPOSER_RESTING_ROWS, composerHeight } from './composer-size.js'

// 15px text at the --leading-body 1.55 ratio, the composer's real metrics.
const line = 23.25
const metrics = { lineHeight: line, padding: 0 }
const resting = Math.ceil(COMPOSER_RESTING_ROWS * line)
const cap = Math.ceil(COMPOSER_MAX_ROWS * line)

describe('composerHeight', () => {
  it('rests at two rows for an empty or single-line draft', () => {
    expect(composerHeight({ ...metrics, contentHeight: 0 })).toEqual({ height: resting, capped: false })
    expect(composerHeight({ ...metrics, contentHeight: line })).toEqual({ height: resting, capped: false })
  })

  it('grows with the measured content while it is below the cap', () => {
    expect(composerHeight({ ...metrics, contentHeight: 5 * line })).toEqual({ height: Math.ceil(5 * line), capped: false })
    expect(composerHeight({ ...metrics, contentHeight: 9 * line })).toEqual({ height: Math.ceil(9 * line), capped: false })
  })

  it('reports the cap as reached at exactly ten lines', () => {
    expect(composerHeight({ ...metrics, contentHeight: COMPOSER_MAX_ROWS * line })).toEqual({ height: cap, capped: true })
  })

  it('clamps above the cap so the textarea scrolls instead of growing', () => {
    expect(composerHeight({ ...metrics, contentHeight: 40 * line })).toEqual({ height: cap, capped: true })
  })

  it('carries the textarea padding into both bounds', () => {
    expect(composerHeight({ lineHeight: line, padding: 8, contentHeight: 0 }).height).toBe(resting + 8)
    expect(composerHeight({ lineHeight: line, padding: 8, contentHeight: 40 * line }).height).toBe(cap + 8)
  })

  it('reports no height when the row height cannot be measured', () => {
    expect(composerHeight({ contentHeight: 400, lineHeight: Number.NaN })).toEqual({ height: null, capped: false })
    expect(composerHeight({ contentHeight: 400, lineHeight: 0 })).toEqual({ height: null, capped: false })
    expect(composerHeight({ contentHeight: 400, lineHeight: -12 })).toEqual({ height: null, capped: false })
  })

  it('falls back to the resting height when the content cannot be measured', () => {
    expect(composerHeight({ ...metrics, contentHeight: Number.NaN })).toEqual({ height: resting, capped: false })
    expect(composerHeight({ ...metrics, contentHeight: undefined })).toEqual({ height: resting, capped: false })
  })

  it('ignores unusable padding and never lets the cap fall below the resting height', () => {
    expect(composerHeight({ ...metrics, contentHeight: 0, padding: Number.NaN }).height).toBe(resting)
    expect(composerHeight({ ...metrics, contentHeight: 40 * line, maxRows: 1 })).toEqual({ height: resting, capped: true })
  })
})

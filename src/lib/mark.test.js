import { describe, expect, it } from 'vitest'

import { MILLED_RING_PATH, ringPath } from './mark.js'

describe('ringPath', () => {
  it('returns the canonical vendored geometry', () => {
    expect(ringPath()).toBe(MILLED_RING_PATH)
    expect(ringPath()).toMatch(/^M40\.50,24\.00 .+ L40\.50,24\.00 Z$/)
  })

  it('preserves all owner-supplied vertices', () => {
    expect(ringPath().match(/\d+\.\d{2},\d+\.\d{2}/g)).toHaveLength(221)
  })
})

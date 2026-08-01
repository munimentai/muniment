import { describe, expect, it } from 'vitest'

import { scrollRegionOverflows } from './scroll-region.js'

describe('scrollRegionOverflows', () => {
  it('reports horizontal overflow', () => {
    expect(scrollRegionOverflows({ scrollWidth: 401, clientWidth: 400 })).toBe(true)
  })

  it('reports no overflow when the content fits', () => {
    expect(scrollRegionOverflows({ scrollWidth: 400, clientWidth: 400 })).toBe(false)
    expect(scrollRegionOverflows({ scrollWidth: 399, clientWidth: 400 })).toBe(false)
  })

  it('reports no overflow for a zero measurement', () => {
    expect(scrollRegionOverflows({ scrollWidth: 0, clientWidth: 0 })).toBe(false)
  })
})

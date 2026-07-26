import { describe, expect, it } from 'vitest'

import { streamingUnderlineGeometry } from './streaming-underline.js'

describe('streamingUnderlineGeometry', () => {
  it('starts at the prose edge and ends at the caret on its active line', () => {
    expect(streamingUnderlineGeometry({ caretLeft: 561, caretTop: 48, caretHeight: 18 })).toEqual({
      left: 0,
      top: 66,
      width: 561,
    })
  })

  it('supports an empty active line without extending past the caret', () => {
    expect(streamingUnderlineGeometry({ caretLeft: 0, caretTop: 24, caretHeight: 18 })).toEqual({
      left: 0,
      top: 42,
      width: 0,
    })
  })

  it.each([
    { caretLeft: Number.NaN, caretTop: 24, caretHeight: 18 },
    { caretLeft: 20, caretTop: Number.POSITIVE_INFINITY, caretHeight: 18 },
    { caretLeft: 20, caretTop: 24, caretHeight: undefined },
    { caretLeft: -1, caretTop: 24, caretHeight: 18 },
    { caretLeft: 20, caretTop: -1, caretHeight: 18 },
    { caretLeft: 20, caretTop: 24, caretHeight: 0 },
  ])('rejects unusable browser measurements: %o', (measurements) => {
    expect(streamingUnderlineGeometry(measurements)).toBeNull()
  })
})

import { describe, expect, it } from 'vitest'

import { forbiddenUiCopy, lintUiCopy } from './ui-copy-lint.mjs'

describe('UI copy lint', () => {
  it('accepts the shipped UI copy', () => {
    expect(lintUiCopy('src')).toEqual([])
  })

  it.each(['AI', 'magic', 'supercharge', 'unlocked', 'sovereignty'])(
    'rejects the forbidden word %s',
    (word) => {
      expect(forbiddenUiCopy(`<button>${word}</button>`)).toEqual([
        { file: '<fixture>', line: 1, word },
      ])
    },
  )

  it('does not reject a word that only contains the same letters', () => {
    expect(forbiddenUiCopy('<p>Mail is available.</p>')).toEqual([])
  })
})

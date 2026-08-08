import { describe, expect, it } from 'vitest'

import { forbiddenEmDashes, forbiddenUiCopy, lintEmDashes, lintUiCopy } from './ui-copy-lint.mjs'

describe('UI copy lint', () => {
  it('accepts the shipped UI copy', () => {
    expect(lintUiCopy('src')).toEqual([])
    expect(lintEmDashes(['src', 'src-tauri', 'browser-control'])).toEqual([])
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

  it.each([
    String.fromCodePoint(0x2014),
    '&mdash;',
    '&#8212;',
    '&#x2014;',
  ])('rejects an em dash written as %s', (emDash) => {
    expect(forbiddenEmDashes(`First clause${emDash}second clause.`)).toEqual([
      { file: '<fixture>', line: 1, word: emDash },
    ])
  })
})

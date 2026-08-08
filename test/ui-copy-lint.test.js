import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { forbiddenEmDashes, forbiddenUiCopy, lintEmDashes, lintUiCopy } from './ui-copy-lint.mjs'

describe('UI copy lint', () => {
  it('accepts the shipped UI copy', () => {
    expect(lintUiCopy('src')).toEqual([])
    expect(lintEmDashes(['src', 'src-tauri', 'browser-control', 'docs/mockups'])).toEqual([])
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
    String.raw`\u2014`,
    String.raw`\u{2014}`,
  ])('rejects an em dash written as %s', (emDash) => {
    expect(forbiddenEmDashes(`First clause${emDash}second clause.`)).toEqual([
      { file: '<fixture>', line: 1, word: emDash },
    ])
  })

  it('rejects an escaped em dash in a nested template', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-copy-lint-'))
    const nested = path.join(root, 'screens', 'mobile')
    fs.mkdirSync(nested, { recursive: true })
    const fixture = path.join(nested, 'template.js')
    fs.writeFileSync(fixture, String.raw`export const copy = "first\u2014second"`)

    try {
      expect(lintEmDashes([root])).toEqual([
        { file: fixture, line: 1, word: String.raw`\u2014` },
      ])
    } finally {
      fs.rmSync(root, { recursive: true, force: true })
    }
  })
})

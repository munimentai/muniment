import { describe, expect, it } from 'vitest'

import { formatBytes, installStateWords } from './speech-install.js'

describe('speech model install display', () => {
  it.each([
    [0, '0 B'],
    [1023, '1023 B'],
    [1024, '1.0 KB'],
    [672_384_307, '641 MB'],
    [940_819_763, '897 MB'],
    [Number.NaN, 'Unknown size'],
    [-1, 'Unknown size'],
  ])('formats %s bytes', (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected)
  })

  it.each([
    ['notInstalled', 'Not installed.'],
    ['installing', 'Installing.'],
    ['installed', 'Installed. Press Voice again to dictate.'],
    ['cancelled', 'Cancelled.'],
    ['failed', 'Failed.'],
    ['unknown', 'Install state unavailable.'],
  ])('maps %s to words', (state, expected) => {
    expect(installStateWords(state)).toBe(expected)
  })
})

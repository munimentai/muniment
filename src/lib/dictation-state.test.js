import { describe, expect, it } from 'vitest'

import { appendTranscript, dictationTransforms, isDictationActive } from './dictation-state.js'

describe('dictation state', () => {
  it('recognizes only starting and running as active', () => {
    expect(isDictationActive({ state: 'starting' })).toBe(true)
    expect(isDictationActive({ state: 'running' })).toBe(true)
    expect(isDictationActive({ state: 'stopped' })).toBe(false)
    expect(isDictationActive({ state: 'failed' })).toBe(false)
  })

  it('appends transcript chunks without disturbing existing whitespace', () => {
    expect(appendTranscript('Existing draft', 'spoken words')).toBe('Existing draft spoken words')
    expect(appendTranscript('Existing draft ', 'spoken words')).toBe('Existing draft spoken words')
    expect(appendTranscript('Existing draft', '')).toBe('Existing draft')
  })

  it('closes transforms to their labels, wire selectors, and advertised shortcuts', () => {
    expect(dictationTransforms).toEqual([
      { label: 'key points', transform: 'key-points', shortcut: 'Alt+1', key: '1' },
      { label: 'formal', transform: 'formal', shortcut: 'Alt+2', key: '2' },
      { label: 'short', transform: 'short', shortcut: 'Alt+3', key: '3' },
      { label: 'long', transform: 'long', shortcut: 'Alt+4', key: '4' },
    ])
  })
})

import { describe, expect, it } from 'vitest'

import { appendTranscript, isDictationActive } from './dictation-state.js'

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
})

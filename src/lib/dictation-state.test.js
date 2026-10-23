import { describe, expect, it } from 'vitest'

import { appendTranscript, ariaKeyShortcut, dictationDraft, handsFreeActivationDelay, holdToTalkShortcut, isDictationActive, shortcutFromKeyboardEvent, validHoldToTalkShortcut } from './dictation-state.js'

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

  it('uses explicit non-system hold-to-talk bindings for each desktop platform', () => {
    expect(holdToTalkShortcut('MacIntel')).toBe('Command+Shift+Space')
    expect(holdToTalkShortcut('Win32')).toBe('Control+Shift+Space')
    expect(holdToTalkShortcut('Linux x86_64')).toBe('Control+Shift+Space')
  })

  it('validates, captures, and exposes voice shortcuts accessibly', () => {
    expect(validHoldToTalkShortcut('Control+Shift+Space')).toBe(true)
    expect(validHoldToTalkShortcut('Command+K')).toBe(true)
    expect(validHoldToTalkShortcut('Space')).toBe(false)
    expect(validHoldToTalkShortcut('Control+Control+K')).toBe(false)
    expect(validHoldToTalkShortcut('Control+Secret')).toBe(false)
    expect(validHoldToTalkShortcut(null)).toBe(false)
    expect(shortcutFromKeyboardEvent({ code: 'KeyK', key: 'k', ctrlKey: true, shiftKey: true })).toBe('Control+Shift+K')
    expect(shortcutFromKeyboardEvent({ code: 'KeyK', key: 'k' })).toBeNull()
    expect(ariaKeyShortcut('Command+Shift+Space')).toBe('Meta+Shift+Space')
  })

  it('uses a short, explicit double-activation window', () => {
    expect(handsFreeActivationDelay).toBe(300)
  })

  it('counts the partial text at the end of the dictation draft', () => {
    expect(dictationDraft('', '', '')).toEqual({ text: '', pending: 0 })
    expect(dictationDraft('Note:', 'One.', '')).toEqual({ text: 'Note: One.', pending: 0 })
    expect(dictationDraft('Note:', 'One.', 'two')).toEqual({ text: 'Note: One. two', pending: 4 })
    expect(dictationDraft('', '', 'two')).toEqual({ text: 'two', pending: 3 })
    expect(dictationDraft('Note: ', '', 'two')).toEqual({ text: 'Note: two', pending: 3 })
  })
})

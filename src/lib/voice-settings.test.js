import { afterEach, describe, expect, it } from 'vitest'
import { VOICE_SETTINGS_KEY, cleanTranscript, dictationStartOptions, readVoiceSettings } from './voice-settings.js'

describe('voice settings', () => {
  afterEach(() => localStorage.clear())

  it('reads the defaults and drops malformed custom words', () => {
    expect(readVoiceSettings()).toMatchObject({ behavior: 'auto', microphone: 'default', mute: true, sound: false, fillers: true, words: [], send: false })
    localStorage.setItem(VOICE_SETTINGS_KEY, JSON.stringify({ behavior: 'hold', words: ['Muniment', 3, ' '] }))
    expect(readVoiceSettings()).toMatchObject({ behavior: 'hold', words: ['Muniment'] })
  })

  it('names a chosen microphone, and mutes only on macOS', () => {
    const settings = { microphone: 'USB Mic', mute: true, sound: true }
    expect(dictationStartOptions(settings, 'MacIntel')).toEqual({ microphone: 'USB Mic', mute: true, sound: true, words: [] })
    expect(dictationStartOptions({ ...settings, microphone: 'default' }, 'Win32')).toEqual({ microphone: null, mute: false, sound: true, words: [] })
  })

  it('passes trimmed custom words to recognition', () => {
    expect(dictationStartOptions({ words: [' Muniment ', '', '  ', 3, 'Zylaxis'] }, 'MacIntel').words).toEqual(['Muniment', 'Zylaxis'])
    expect(dictationStartOptions({ words: 'Muniment' }, 'MacIntel').words).toEqual([])
  })

  it('drops filler words and keeps the sentence shape', () => {
    const fillers = { fillers: true }
    expect(cleanTranscript('Um, so I think, uh, we should go.', fillers)).toBe('So I think, we should go.')
    expect(cleanTranscript('It works. Uh, mostly.', fillers)).toBe('It works. Mostly.')
    expect(cleanTranscript('I went there uh.', fillers)).toBe('I went there.')
    expect(cleanTranscript('Umm.', fillers)).toBe('')
    expect(cleanTranscript('Uh-huh, bring the umbrella and hum.', fillers)).toBe('Uh-huh, bring the umbrella and hum.')
    expect(cleanTranscript('Um, so.', { fillers: false })).toBe('Um, so.')
    expect(cleanTranscript('\nfirst line\nsecond line', fillers)).toBe('\nfirst line\nsecond line')
  })

  it('spells custom words the way the list does', () => {
    expect(cleanTranscript('ask muniment about the parakeet model, not munimentai', { words: ['Muniment', 'Parakeet'] })).toBe('ask Muniment about the Parakeet model, not munimentai')
    expect(cleanTranscript('call c++ now', { words: ['C++'] })).toBe('call C++ now')
  })
})

import { readDevicePreference, saveDevicePreference } from './device-preferences.js'

// Voice choices live on this device. Dictation reads them at each start, so a
// change applies to the next dictation.
export const VOICE_SETTINGS_KEY = 'muniment.voice'
export const VOICE_DEFAULTS = { behavior: 'auto', microphone: 'default', mute: true, sound: false, fillers: true, words: [], send: false }

export function readVoiceSettings() {
  const settings = readDevicePreference(VOICE_SETTINGS_KEY, VOICE_DEFAULTS)
  return { ...settings, words: Array.isArray(settings.words) ? settings.words.filter((word) => typeof word === 'string' && word.trim()) : [] }
}

export function saveVoiceSettings(settings) {
  return saveDevicePreference(VOICE_SETTINGS_KEY, settings)
}

// The options dictation_start takes. Muting is a macOS feature. Custom words
// bias recognition, and cleanTranscript still fixes their casing.
export function dictationStartOptions(settings, platform = navigator.platform) {
  return {
    microphone: settings.microphone && settings.microphone !== 'default' ? settings.microphone : null,
    mute: !!settings.mute && platform.startsWith('Mac'),
    sound: !!settings.sound,
    words: Array.isArray(settings.words) ? settings.words.filter((word) => typeof word === 'string').map((word) => word.trim()).filter(Boolean) : [],
  }
}

// A filler is a whole word, so "uh-huh", "umbrella" and "hum" stay.
const FILLER = /(?<![\p{L}\p{N}'-])(?:u+m+|u+h+|e+r+m+|h+m+)(?![\p{L}\p{N}'-]),?[ \t]*/giu
const SENTENCE_START = /(?:^|[.!?]\s*)$/
const escapeWord = (word) => word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

export function cleanTranscript(text, { fillers = false, words = [] } = {}) {
  let next = text
  if (fillers) {
    next = next
      .replace(FILLER, (_, offset, whole) => (SENTENCE_START.test(whole.slice(0, offset)) ? '\u0000' : ''))
      .replace(/\u0000[ \t]*(\p{Ll})/gu, (_, letter) => letter.toUpperCase())
      .replaceAll('\u0000', '')
      .replace(/[ \t]+([,.!?;:])/g, '$1')
      .replace(/([,;:]){2,}/g, '$1')
      .replace(/^[ \t,;:]+/, '')
      .replace(/[ \t]{2,}/g, ' ')
      .replace(/[ \t]+$/, '')
    if (!/[\p{L}\p{N}]/u.test(next)) next = ''
  }
  for (const word of words) {
    next = next.replace(new RegExp(`(?<![\\p{L}\\p{N}])${escapeWord(word.trim())}(?![\\p{L}\\p{N}])`, 'giu'), word.trim())
  }
  return next
}

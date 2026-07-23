export const activeDictationStates = new Set(['starting', 'running'])
export const handsFreeActivationDelay = 300

export const dictationTransforms = [
  { label: 'key points', transform: 'key-points', shortcut: 'Alt+1', key: '1' },
  { label: 'formal', transform: 'formal', shortcut: 'Alt+2', key: '2' },
  { label: 'short', transform: 'short', shortcut: 'Alt+3', key: '3' },
  { label: 'long', transform: 'long', shortcut: 'Alt+4', key: '4' },
]

// Avoid macOS Spotlight (Command+Space), Windows-key combinations, and other
// documented system bindings. A collision can still occur with another app.
export function holdToTalkShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Command+Shift+Space' : 'Control+Shift+Space'
}

const shortcutModifiers = ['Command', 'Control', 'Alt', 'Shift']
const shortcutKeys = /^(?:[A-Z]|[0-9]|F(?:[1-9]|1[0-9]|2[0-4])|Space|Enter|Tab|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Arrow(?:Up|Down|Left|Right))$/

export function validHoldToTalkShortcut(value) {
  if (typeof value !== 'string') return false
  const parts = value.split('+')
  const key = parts.pop()
  return parts.length > 0 && new Set(parts).size === parts.length && parts.every((part) => shortcutModifiers.includes(part)) && shortcutKeys.test(key)
}

export function shortcutFromKeyboardEvent(event) {
  const key = event.code?.startsWith('Key') ? event.code.slice(3)
    : event.code?.startsWith('Digit') ? event.code.slice(5)
      : event.code === 'Space' ? 'Space' : event.key
  if (!shortcutKeys.test(key) || !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) return null
  return [event.metaKey && 'Command', event.ctrlKey && 'Control', event.altKey && 'Alt', event.shiftKey && 'Shift', key].filter(Boolean).join('+')
}

export function ariaKeyShortcut(shortcut) {
  return shortcut.replace('Command', 'Meta')
}

export function isDictationActive(status) {
  return activeDictationStates.has(status?.state)
}

export function appendTranscript(draft, text) {
  if (!text) return draft
  if (!draft || /\s$/.test(draft) || /^\s/.test(text)) return `${draft}${text}`
  return `${draft} ${text}`
}

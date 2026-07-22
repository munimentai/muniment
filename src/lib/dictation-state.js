export const activeDictationStates = new Set(['starting', 'running'])

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

export function isDictationActive(status) {
  return activeDictationStates.has(status?.state)
}

export function appendTranscript(draft, text) {
  if (!text) return draft
  if (!draft || /\s$/.test(draft) || /^\s/.test(text)) return `${draft}${text}`
  return `${draft} ${text}`
}

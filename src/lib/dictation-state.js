export const activeDictationStates = new Set(['starting', 'running'])

export function isDictationActive(status) {
  return activeDictationStates.has(status?.state)
}

export function appendTranscript(draft, text) {
  if (!text) return draft
  if (!draft || /\s$/.test(draft) || /^\s/.test(text)) return `${draft}${text}`
  return `${draft} ${text}`
}

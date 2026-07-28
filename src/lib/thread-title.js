const MAX_TITLE_SCALARS = 80
const EMPTY_THREAD_TITLE = 'New thread'

export function threadTitle(messages) {
  const prompt = messages.find((message) => message?.role === 'user')?.text
  if (typeof prompt !== 'string') return EMPTY_THREAD_TITLE

  const normalized = prompt.trim().split(/\s+/u).join(' ')
  if (!normalized) return EMPTY_THREAD_TITLE

  const scalars = Array.from(normalized)
  if (scalars.length <= MAX_TITLE_SCALARS) return normalized
  return `${scalars.slice(0, MAX_TITLE_SCALARS - 1).join('')}…`
}

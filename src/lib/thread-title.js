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

// Creation goals are instructions, not names. Wait for the shared thread namer
// rather than displaying a paragraph while an artifact or agent is built.
export function creationTitle(creation, summaries = [], name) {
  if (name?.trim()) return name.trim()
  const title = summaries.find(item => item.threadId === creation?.threadId)?.title
  for (const value of [title, creation?.goal]) {
    const candidate = value?.trim().split(/\s+/u).join(' ')
    if (candidate && candidate.split(' ').length <= 3 && Array.from(candidate).length <= 80) return candidate
  }
  return creation?.kind === 'agent' ? 'New agent' : 'New artifact'
}

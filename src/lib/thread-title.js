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

// The scaffold an older creation chat sent as its first message.
const SCAFFOLD = /^I want to create an? (agent|artifact) in this dedicated chat\b/u

// The thread list's name for a creation chat, the shared thread namer's name
// or the user's own first words, when it can name the chat. Goals are
// instructions, not names, and an older chat's scaffold never shows.
export function creationThreadTitle(creation, summaries = []) {
  const goal = normal(creation?.goal)
  const title = normal(summaries.find(item => item.threadId === creation?.threadId)?.title)
  return title && title !== goal && !SCAFFOLD.test(title) && Array.from(title).length <= 80 ? title : ''
}

function normal(text) {
  return typeof text === 'string' ? text.trim().split(/\s+/u).join(' ') : ''
}

// A creation chat's title: its saved name, then the thread list's name, then
// the name the record keeps for when the list no longer holds the chat, then
// the newest draft's name, then a short goal.
export function creationTitle(creation, summaries = [], name) {
  if (name?.trim()) return name.trim()
  const title = creationThreadTitle(creation, summaries) || normal(creation?.title)
  if (title && Array.from(title).length <= 80) return title
  const draft = normal(creation?.turns?.findLast?.(turn => normal(turn?.reply?.draft?.name))?.reply?.draft?.name)
  if (draft && Array.from(draft).length <= 80) return draft
  const goal = normal(creation?.goal)
  if (goal && goal.split(' ').length <= 3 && Array.from(goal).length <= 80) return goal
  return creation?.kind === 'agent' ? 'New agent' : 'New artifact'
}

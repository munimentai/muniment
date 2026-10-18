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

// A creation chat's title: its saved name, then the thread's title, which is
// the shared thread namer's name or the user's own first words, then a short
// goal. Goals are instructions, not names, and an older chat's scaffold never
// shows.
export function creationTitle(creation, summaries = [], name) {
  if (name?.trim()) return name.trim()
  const goal = creation?.goal?.trim().split(/\s+/u).join(' ')
  const title = summaries.find(item => item.threadId === creation?.threadId)?.title?.trim().split(/\s+/u).join(' ')
  if (title && title !== goal && !SCAFFOLD.test(title) && Array.from(title).length <= 80) return title
  if (goal && goal.split(' ').length <= 3 && Array.from(goal).length <= 80) return goal
  return creation?.kind === 'agent' ? 'New agent' : 'New artifact'
}

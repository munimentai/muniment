export const THREAD_ORGANIZATION_KEY = 'muniment.thread-organization'

export function readThreadOrganization(storage = localStorage) {
  try {
    const value = JSON.parse(storage.getItem(THREAD_ORGANIZATION_KEY) || '{}')
    return Object.fromEntries(Object.entries(value).filter(([, entry]) => entry && typeof entry === 'object')
      .map(([id, entry]) => [id, { pinned: entry.pinned === true, archived: entry.archived === true }]))
  } catch (_) {
    return {}
  }
}

export function organizeThreads(summaries, organization, query, archivedOnly, includeAll = false) {
  const search = query.trim().toLocaleLowerCase()
  const groups = { Pinned: [], Threads: [], Archived: [] }
  for (const summary of summaries) {
    const state = organization[summary.threadId] || {}
    if (search && !(summary.title || 'New thread').toLocaleLowerCase().includes(search)) continue
    if (!includeAll && (archivedOnly ? !state.archived : state.archived && !search)) continue
    groups[state.archived ? 'Archived' : state.pinned ? 'Pinned' : 'Threads'].push(summary)
  }
  return Object.entries(groups).filter(([, threads]) => threads.length).map(([name, threads]) => ({ name, threads }))
}

// The threads the sidebar's Threads list holds. A group's thread and an
// agent's own conversations live under Agents. A chat that made an artifact
// or an agent stays a thread the user can find, beside its catalog entry,
// whether the user started it from a catalog or the model saved the result
// mid-chat. `creations` are core's creation records, `{ threadId }` each.
export function listedThreads(summaries, { agentThreads = {}, groupThreads = [], creations = [] } = {}) {
  const groups = new Set(groupThreads)
  const created = new Set(creations.map((item) => item.threadId).filter(Boolean))
  return summaries.filter((thread) => !groups.has(thread.threadId) && (!agentThreads[thread.threadId] || created.has(thread.threadId)))
}

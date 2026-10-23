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

// The threads the Projects catalog lists. A group's thread and an
// agent's own conversations live with the group and the agent. A chat that made an artifact
// or an agent stays a thread the user can find, beside its catalog entry,
// whether the user started it from a catalog or the model saved the result
// mid-chat. `creations` are core's creation records, `{ threadId }` each.
export function listedThreads(summaries, { agentThreads = {}, groupThreads = [], creations = [] } = {}) {
  const groups = new Set(groupThreads)
  const created = new Set(creations.map((item) => item.threadId).filter(Boolean))
  return summaries.filter((thread) => !groups.has(thread.threadId) && (!agentThreads[thread.threadId] || created.has(thread.threadId)))
}

export const THREAD_ORGANIZATION_KEY = 'muniment.thread-organization'
// The thread open at quit, which launch opens again. Absent while a new
// thread or no thread shows.
export const OPEN_THREAD_KEY = 'muniment.open-thread'

export function readOpenThread(storage = localStorage) {
  try { return storage.getItem(OPEN_THREAD_KEY) || null } catch (_) { return null }
}

export function keepOpenThread(threadId, storage = localStorage) {
  try {
    if (threadId) storage.setItem(OPEN_THREAD_KEY, threadId)
    else storage.removeItem(OPEN_THREAD_KEY)
  } catch (_) {}
}

// Launch never reopens a thread the user deleted.
export function forgetOpenThread(threadIds, storage = localStorage) {
  const open = readOpenThread(storage)
  if (open && threadIds.includes(open)) keepOpenThread(null, storage)
}

// The thread that shows after `threadIds` are deleted: the newest other
// thread in `summaries` that is not archived, else null for a new thread.
export function threadAfterDelete(summaries, threadIds, organization = {}) {
  const gone = new Set(threadIds)
  return summaries.find((thread) => !gone.has(thread.threadId) && !organization[thread.threadId]?.archived)?.threadId ?? null
}

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

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

export function organizeThreads(summaries, organization, query, archivedOnly) {
  const search = query.trim().toLocaleLowerCase()
  const groups = { Pinned: [], Threads: [], Archived: [] }
  for (const summary of summaries) {
    const state = organization[summary.threadId] || {}
    if (search && !(summary.title || 'New thread').toLocaleLowerCase().includes(search)) continue
    if (archivedOnly ? !state.archived : state.archived && !search) continue
    groups[state.archived ? 'Archived' : state.pinned ? 'Pinned' : 'Threads'].push(summary)
  }
  return Object.entries(groups).filter(([, threads]) => threads.length).map(([name, threads]) => ({ name, threads }))
}

// A saved memory's Source line in words: core's `origin_label`, such as
// Added in a chat, and the thread the fact came from by its title, which
// opens it. A fact written before origins carries `thread:<id> run:<id>` as
// its source, and its thread comes from that text.
const LEGACY_RUN = /^(?:extracted )?thread:(\S+)(?: run:\S+)?$/

export function factThread(fact) {
  return fact?.origin?.thread_id || LEGACY_RUN.exec(String(fact?.source ?? '').trim())?.[1] || ''
}

export function sourceLabel(fact) {
  if (typeof fact?.origin_label === 'string' && fact.origin_label.trim()) return fact.origin_label.trim()
  if (LEGACY_RUN.test(String(fact?.source ?? '').trim())) return 'Added in a chat'
  return fact?.source ?? ''
}

// The titles of the threads `ids` names, from the thread list one page at a
// time, until every one is found or the list ends. A thread the list does
// not hold, such as a deleted one, gets no title.
const PAGE = 100
const PAGES = 20
export async function threadTitles(tauri, ids) {
  const wanted = new Set(ids.filter(Boolean))
  const titles = new Map()
  let cursor
  for (let page = 0; wanted.size && page < PAGES; page += 1) {
    const payload = { limit: PAGE }
    if (cursor !== undefined) payload.cursor = cursor
    const result = await tauri.invoke('chat_thread_summaries', payload)
    for (const summary of result?.summaries ?? []) {
      if (!wanted.delete(summary.threadId)) continue
      titles.set(summary.threadId, summary.title?.trim() || 'New thread')
    }
    if (result?.nextCursor == null) break
    cursor = result.nextCursor
  }
  return titles
}

// Settings opens a thread through the shell, which owns the thread view.
export const OPEN_THREAD_EVENT = 'muniment:open-thread'
export function openThread(threadId) {
  try { window.dispatchEvent(new CustomEvent(OPEN_THREAD_EVENT, { detail: { threadId } })) } catch (_) {}
}

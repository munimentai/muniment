// The threads this computer knows were deleted: the ones the local runtime's
// thread list names, and any a run link failed to open because the runtime
// no longer finds it. A row that opens a run's thread reads it, so a run
// whose thread is gone says so instead of opening nothing.
import { SvelteSet } from 'svelte/reactivity'

export const DELETED_THREAD_NOTE = "This run's thread was deleted."

// The shell's words when the runtime does not find a selected thread.
const NOT_FOUND = 'Muniment cannot find this thread. Open another thread.'

const deleted = new SvelteSet()

// Every known deleted thread, sorted, for a request that names them.
export function deletedThreadIds() {
  return [...deleted].sort()
}

export function threadDeleted(threadId) {
  return !!threadId && deleted.has(threadId)
}

// A deleted thread never returns, so each list adds to what is known.
export function rememberDeletedThreads(threadIds) {
  for (const threadId of Array.isArray(threadIds) ? threadIds : []) {
    if (typeof threadId === 'string' && threadId) deleted.add(threadId)
  }
}

export function markThreadDeleted(threadId) {
  if (threadId) deleted.add(threadId)
}

// Whether a failed select or history read says the thread is gone.
export function threadMissing(error) {
  return String(error?.message ?? error ?? '') === NOT_FOUND
}

export function forgetDeletedThreads() {
  deleted.clear()
}

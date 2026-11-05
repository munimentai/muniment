// The workspace panel belongs to threads. Leaving the thread view for
// Settings or a catalog page puts the open tool aside, and coming back opens
// it again. A tool the user opens on the other page stays, and it wins over
// the one put aside.
export function workspaceOnNavigate({ panel = null, parked = null } = {}, threadView) {
  if (!threadView) return panel ? { panel: null, parked: panel } : { panel, parked }
  return { panel: panel ?? parked, parked: null }
}

// The agent profile beside a thread belongs to that thread. `shown` names the
// thread whose profile shows in the thread view. Leaving the view puts it
// aside, and coming back to the same thread opens it again. Another thread,
// or a profile the user opened meanwhile, drops the one put aside. While a
// thread opens, the thread that shows is not settled, so nothing changes.
export function profileOnNavigate({ shown = null, parked = null } = {}, { threadView, threadId = null, open = false, switching = false }) {
  if (!threadView) return { shown: null, parked: parked ?? shown, restore: false }
  if (switching) return { shown, parked, restore: false }
  const restore = !open && !!parked && parked === threadId
  return { shown: open || restore ? threadId : null, parked: null, restore }
}

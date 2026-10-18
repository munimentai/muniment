// The workspace panel belongs to threads. Leaving the thread view for
// Settings or a catalog page puts the open tool aside, and coming back opens
// it again. A tool the user opens on the other page stays, and it wins over
// the one put aside.
export function workspaceOnNavigate({ panel = null, parked = null } = {}, threadView) {
  if (!threadView) return panel ? { panel: null, parked: panel } : { panel, parked }
  return { panel: panel ?? parked, parked: null }
}

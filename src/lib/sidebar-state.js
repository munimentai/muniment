export const sidebarStorageKey = 'muniment.sidebar-collapsed'

export function readSidebarCollapsed(storage) {
  try {
    return storage?.getItem(sidebarStorageKey) === 'true'
  } catch {
    return false
  }
}

export function writeSidebarCollapsed(storage, collapsed) {
  try {
    storage?.setItem(sidebarStorageKey, String(collapsed))
  } catch {
    // Storage can be unavailable without preventing the sidebar from working.
  }
}

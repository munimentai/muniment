// design-spec §2.1: expanded by default (260px), state remembered, collapse
// (⌘\) animates to a 52px icon rail.
export const SIDEBAR_STORAGE_KEY = 'muniment.sidebar-collapsed'
export const SIDEBAR_COLLAPSED = 'collapsed'
export const SIDEBAR_EXPANDED = 'expanded'

export function sidebarShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+\\' : 'Control+\\'
}

export function isSidebarShortcut(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  return event.key === '\\'
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !event.shiftKey
}

// Anything but an explicit collapsed marker — missing, malformed, or written
// by an older shell — restores the documented default.
export function parseSidebarCollapsed(stored) {
  return stored === SIDEBAR_COLLAPSED
}

export function serializeSidebarCollapsed(collapsed) {
  return collapsed ? SIDEBAR_COLLAPSED : SIDEBAR_EXPANDED
}

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

export function newThreadShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+N' : 'Control+N'
}

export function isNewThreadShortcut(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  return event.key.toLowerCase() === 'n'
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !event.shiftKey
}

export function threadRowShortcut(position, platform = navigator.platform) {
  return `${platform.startsWith('Mac') ? 'Meta' : 'Control'}+${position}`
}

export function threadRowShortcutPosition(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  const match = /^Digit([1-9])$/.exec(event.code)
  if (!match
    || (mac ? !event.metaKey || event.ctrlKey : !event.ctrlKey || event.metaKey)
    || event.altKey
    || event.shiftKey) return null
  return Number(match[1])
}

// A missing, malformed, or older value restores the documented default.
export function parseSidebarCollapsed(stored) {
  return stored === SIDEBAR_COLLAPSED
}

export function storedSidebarCollapsed(storage = localStorage) {
  try {
    return parseSidebarCollapsed(storage.getItem(SIDEBAR_STORAGE_KEY))
  } catch (_) {
    return false
  }
}

export function serializeSidebarCollapsed(collapsed) {
  return collapsed ? SIDEBAR_COLLAPSED : SIDEBAR_EXPANDED
}

// The sidebar resizes like the artifact rail: a pointer drag on its divider,
// arrow keys on the separator, and the width kept per device.
export const SIDEBAR_WIDTH_STORAGE_KEY = 'muniment.sidebar-width'
export const SIDEBAR_RAIL_WIDTH = 52
export const SIDEBAR_MIN_WIDTH = 160
export const SIDEBAR_MAX_WIDTH = 420
export const SIDEBAR_DEFAULT_WIDTH = 195
export const SIDEBAR_KEYBOARD_STEP = 20

export function clampSidebarWidth(width, maximum = SIDEBAR_MAX_WIDTH) {
  const upper = Number.isFinite(maximum) ? Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, maximum)) : SIDEBAR_MAX_WIDTH
  const candidate = Number.isFinite(width) ? width : SIDEBAR_DEFAULT_WIDTH
  return Math.round(Math.min(upper, Math.max(SIDEBAR_MIN_WIDTH, candidate)))
}

export function sidebarWidthFromPointer(clientX, leftEdge, maximum = SIDEBAR_MAX_WIDTH) {
  return clampSidebarWidth(clientX - leftEdge, maximum)
}

export function sidebarWidthFromKey(width, key, maximum = SIDEBAR_MAX_WIDTH) {
  if (key === 'Home') return SIDEBAR_MIN_WIDTH
  if (key === 'End') return clampSidebarWidth(maximum, maximum)
  if (key === 'ArrowRight') return clampSidebarWidth(width + SIDEBAR_KEYBOARD_STEP, maximum)
  if (key === 'ArrowLeft') return clampSidebarWidth(width - SIDEBAR_KEYBOARD_STEP, maximum)
  return width
}

export function storedSidebarWidth(storage = localStorage) {
  try {
    return clampSidebarWidth(Number(storage.getItem(SIDEBAR_WIDTH_STORAGE_KEY) ?? Number.NaN))
  } catch (_) {
    return SIDEBAR_DEFAULT_WIDTH
  }
}

export function createSidebarResizeController({ readWidth, readMaximum, readPointer, readAvailableWidth, readLeftEdge, onWidth, onMaximum, onPointer, storage }) {
  // The storage is read inside the guard: a page with no storage still resizes.
  function setWidth(width) {
    onWidth(width)
    try { (storage ?? localStorage).setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(width)) } catch (_) {}
  }

  function fit() {
    const maximum = readAvailableWidth()
    onMaximum(maximum)
    onWidth(clampSidebarWidth(readWidth(), maximum))
  }

  function pointerDown(event) {
    if (event.button !== 0 || readPointer() !== undefined) return
    event.preventDefault()
    onPointer(event.pointerId)
    event.currentTarget.setPointerCapture?.(event.pointerId)
  }

  function pointerMove(event) {
    if (event.pointerId !== readPointer()) return
    setWidth(sidebarWidthFromPointer(event.clientX, readLeftEdge(), readMaximum()))
  }

  function pointerEnd(event) {
    if (event.pointerId !== readPointer()) return
    onPointer(undefined)
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
  }

  function keydown(event) {
    const width = sidebarWidthFromKey(readWidth(), event.key, readMaximum())
    if (width === readWidth() && !['Home', 'End', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return
    event.preventDefault()
    setWidth(width)
  }

  return { fit, pointerDown, pointerMove, pointerEnd, keydown }
}

export const ARTIFACT_RAIL_MIN_WIDTH = 380
export const ARTIFACT_RAIL_MAX_WIDTH = 560
export const ARTIFACT_RAIL_KEYBOARD_STEP = 20

export function clampArtifactRailWidth(width, maximum = ARTIFACT_RAIL_MAX_WIDTH) {
  const upper = Number.isFinite(maximum) ? Math.max(ARTIFACT_RAIL_MIN_WIDTH, maximum) : ARTIFACT_RAIL_MAX_WIDTH
  const candidate = Number.isFinite(width) ? width : ARTIFACT_RAIL_MIN_WIDTH
  return Math.round(Math.min(upper, Math.max(ARTIFACT_RAIL_MIN_WIDTH, candidate)))
}

export function defaultArtifactRailWidth(viewportWidth) {
  return clampArtifactRailWidth(viewportWidth * 0.34)
}

export function artifactRailWidthFromPointer(clientX, rightEdge, maximum = ARTIFACT_RAIL_MAX_WIDTH) {
  return clampArtifactRailWidth(rightEdge - clientX, maximum)
}

export function artifactRailWidthFromKey(width, key, maximum = ARTIFACT_RAIL_MAX_WIDTH) {
  if (key === 'Home') return ARTIFACT_RAIL_MIN_WIDTH
  if (key === 'End') return clampArtifactRailWidth(maximum, maximum)
  if (key === 'ArrowLeft') return clampArtifactRailWidth(width + ARTIFACT_RAIL_KEYBOARD_STEP, maximum)
  if (key === 'ArrowRight') return clampArtifactRailWidth(width - ARTIFACT_RAIL_KEYBOARD_STEP, maximum)
  return width
}

export function artifactRailShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+J' : 'Control+J'
}

export function shortcutDisplayLabel(shortcut) {
  if (shortcut.startsWith('Meta+')) return `⌘${shortcut.slice('Meta+'.length)}`
  if (shortcut.startsWith('Control+')) return `Ctrl ${shortcut.slice('Control+'.length)}`
  return shortcut
}

export function isArtifactRailShortcut(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  return event.key.toLowerCase() === 'j'
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !event.shiftKey
}

export function createArtifactRailController({
  readOpen,
  readWidth,
  readMaximum,
  readPointer,
  readAvailableWidth,
  readRightEdge,
  readViewportWidth,
  onOpen,
  onWidth,
  onMaximum,
  onPointer,
}) {
  function resetWidth() {
    const maximum = readAvailableWidth()
    onMaximum(maximum)
    onWidth(clampArtifactRailWidth(defaultArtifactRailWidth(readViewportWidth()), maximum))
  }

  function fit() {
    if (!readOpen()) return
    const maximum = readAvailableWidth()
    onMaximum(maximum)
    onWidth(clampArtifactRailWidth(readWidth(), maximum))
  }

  function toggle() {
    const open = !readOpen()
    onOpen(open)
    onPointer(undefined)
    if (open) resetWidth()
  }

  function pointerDown(event) {
    if (event.button !== 0 || readPointer() !== undefined) return
    event.preventDefault()
    event.currentTarget.focus()
    onPointer(event.pointerId)
    event.currentTarget.setPointerCapture?.(event.pointerId)
  }

  function pointerMove(event) {
    if (event.pointerId !== readPointer()) return
    onWidth(artifactRailWidthFromPointer(event.clientX, readRightEdge(), readMaximum()))
  }

  function pointerEnd(event) {
    if (event.pointerId !== readPointer()) return
    onPointer(undefined)
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
  }

  function keydown(event) {
    const width = artifactRailWidthFromKey(readWidth(), event.key, readMaximum())
    if (width === readWidth() && !['Home', 'End', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return
    event.preventDefault()
    onWidth(width)
  }

  return { fit, toggle, pointerDown, pointerMove, pointerEnd, keydown }
}

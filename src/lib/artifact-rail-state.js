// One rail column at the right of the workspace, and two panels that can fill
// it: the artifact rail and the record panel. Opening one closes the other.
// Each occupant carries its own width bounds, and the record panel can
// maximize over the sidebar and the thread until the shortcut or Escape
// restores them.

export const ARTIFACT_RAIL_MIN_WIDTH = 380
export const ARTIFACT_RAIL_MAX_WIDTH = 560
export const RECORD_PANEL_MIN_WIDTH = 480
export const RECORD_PANEL_MAX_WIDTH = 960
export const ARTIFACT_RAIL_KEYBOARD_STEP = 20

export const RAIL_OCCUPANTS = Object.freeze({
  artifacts: Object.freeze({ min: ARTIFACT_RAIL_MIN_WIDTH, max: ARTIFACT_RAIL_MAX_WIDTH, share: 0.34 }),
  files: Object.freeze({ min: RECORD_PANEL_MIN_WIDTH, max: RECORD_PANEL_MAX_WIDTH, share: 0.5 }),
  record: Object.freeze({ min: RECORD_PANEL_MIN_WIDTH, max: RECORD_PANEL_MAX_WIDTH, share: 0.5 }),
})

export function railBounds(occupant) {
  return RAIL_OCCUPANTS[occupant] ?? RAIL_OCCUPANTS.artifacts
}

export function clampRailWidth(width, occupant = 'artifacts', maximum) {
  const bounds = railBounds(occupant)
  const upper = Number.isFinite(maximum) ? Math.max(bounds.min, maximum) : bounds.max
  const candidate = Number.isFinite(width) ? width : bounds.min
  return Math.round(Math.min(upper, Math.max(bounds.min, candidate)))
}

export function defaultRailWidth(viewportWidth, occupant = 'artifacts') {
  return clampRailWidth(viewportWidth * railBounds(occupant).share, occupant)
}

export function railWidthFromPointer(clientX, rightEdge, occupant = 'artifacts', maximum) {
  return clampRailWidth(rightEdge - clientX, occupant, maximum)
}

export function railWidthFromKey(width, key, occupant = 'artifacts', maximum) {
  const bounds = railBounds(occupant)
  if (key === 'Home') return bounds.min
  if (key === 'End') return clampRailWidth(maximum ?? bounds.max, occupant, maximum)
  if (key === 'ArrowLeft') return clampRailWidth(width + ARTIFACT_RAIL_KEYBOARD_STEP, occupant, maximum)
  if (key === 'ArrowRight') return clampRailWidth(width - ARTIFACT_RAIL_KEYBOARD_STEP, occupant, maximum)
  return width
}

// The artifact rail's own names stay, so a caller that knows one occupant reads as before.
export function clampArtifactRailWidth(width, maximum = ARTIFACT_RAIL_MAX_WIDTH) {
  return clampRailWidth(width, 'artifacts', maximum)
}

export function defaultArtifactRailWidth(viewportWidth) {
  return defaultRailWidth(viewportWidth, 'artifacts')
}

export function artifactRailWidthFromPointer(clientX, rightEdge, maximum = ARTIFACT_RAIL_MAX_WIDTH) {
  return railWidthFromPointer(clientX, rightEdge, 'artifacts', maximum)
}

export function artifactRailWidthFromKey(width, key, maximum = ARTIFACT_RAIL_MAX_WIDTH) {
  return railWidthFromKey(width, key, 'artifacts', maximum)
}

export function artifactRailShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+J' : 'Control+J'
}

export function recordPanelShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+K' : 'Control+K'
}

export function shortcutDisplayLabel(shortcut) {
  // One space between the modifier and the key on every platform, so the chip reads as two parts.
  if (shortcut.startsWith('Meta+')) return `⌘ ${shortcut.slice('Meta+'.length)}`
  if (shortcut.startsWith('Control+')) return `Ctrl ${shortcut.slice('Control+'.length)}`
  return shortcut
}

function isPlainSuperKey(event, key, platform) {
  const mac = platform.startsWith('Mac')
  return event.key.toLowerCase() === key
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !event.shiftKey
}

export function isArtifactRailShortcut(event, platform = navigator.platform) {
  return isPlainSuperKey(event, 'j', platform)
}

export function isRecordPanelShortcut(event, platform = navigator.platform) {
  return isPlainSuperKey(event, 'k', platform)
}

// The one rail column's controller. `readOccupant` answers null, 'artifacts'
// or 'record'. The record panel alone maximizes.
export function createRailController({
  readOccupant,
  onOccupant,
  readWidth,
  onWidth,
  readMaximum,
  onMaximum,
  readPointer,
  onPointer,
  readAvailableWidth,
  readRightEdge,
  readViewportWidth,
  readMaximized = () => false,
  onMaximized = () => {},
}) {
  function resetWidth(occupant) {
    const maximum = readAvailableWidth(occupant)
    onMaximum(maximum)
    onWidth(clampRailWidth(defaultRailWidth(readViewportWidth(), occupant), occupant, maximum))
  }

  function fit() {
    const occupant = readOccupant()
    if (!occupant) return
    const maximum = readAvailableWidth(occupant)
    onMaximum(maximum)
    onWidth(clampRailWidth(readWidth(), occupant, maximum))
  }

  function open(occupant) {
    onPointer(undefined)
    onMaximized(false)
    onOccupant(occupant)
    resetWidth(occupant)
  }

  function close() {
    onPointer(undefined)
    onMaximized(false)
    onOccupant(null)
  }

  function toggle(occupant) {
    if (readOccupant() === occupant) close()
    else open(occupant)
  }

  function toggleMaximized() {
    if (!['record', 'files'].includes(readOccupant())) return
    onMaximized(!readMaximized())
  }

  function pointerDown(event) {
    if (event.button !== 0 || readPointer() !== undefined) return
    event.preventDefault()
    onPointer(event.pointerId)
    event.currentTarget.setPointerCapture?.(event.pointerId)
  }

  function pointerMove(event) {
    if (event.pointerId !== readPointer()) return
    onWidth(railWidthFromPointer(event.clientX, readRightEdge(), readOccupant(), readMaximum()))
  }

  function pointerEnd(event) {
    if (event.pointerId !== readPointer()) return
    onPointer(undefined)
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
  }

  function keydown(event) {
    const width = railWidthFromKey(readWidth(), event.key, readOccupant(), readMaximum())
    if (width === readWidth() && !['Home', 'End', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return
    event.preventDefault()
    onWidth(width)
  }

  return { fit, open, close, toggle, toggleMaximized, pointerDown, pointerMove, pointerEnd, keydown }
}

// The artifact rail's controller of one occupant, kept for callers that read `open`.
export function createArtifactRailController({ readOpen, onOpen, readAvailableWidth, ...rest }) {
  const controller = createRailController({
    ...rest,
    readOccupant: () => (readOpen() ? 'artifacts' : null),
    onOccupant: (next) => onOpen(next === 'artifacts'),
    readAvailableWidth: () => readAvailableWidth(),
  })
  return {
    fit: controller.fit,
    toggle: () => controller.toggle('artifacts'),
    pointerDown: controller.pointerDown,
    pointerMove: controller.pointerMove,
    pointerEnd: controller.pointerEnd,
    keydown: controller.keydown,
  }
}

export function artifactRailShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+J' : 'Control+J'
}

export function isEditableTarget(target) {
  return target instanceof Element
    && (target.matches('input, textarea') || target.closest('[contenteditable]:not([contenteditable="false"])') !== null)
}

export function isArtifactRailShortcut(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  return event.key.toLowerCase() === 'j'
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !event.shiftKey
    && !isEditableTarget(event.target)
}

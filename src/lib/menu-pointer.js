// Focus follows the pointer in a menu, as in a native menu: the item under the
// pointer takes focus, so the item a menu focused on open, or the one the keys
// last reached, never keeps its fill beside the hovered one. base.css hides the
// hover fill of an item without focus while the keys lead.
const ITEM = '[role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"], [role="option"]'

export function followPointer(node) {
  const move = (event) => {
    const item = event.target?.closest?.(ITEM)
    if (!item || !node.contains(item) || item.disabled || item === document.activeElement) return
    item.focus({ preventScroll: true })
  }
  node.addEventListener('pointermove', move)
  return { destroy() { node.removeEventListener('pointermove', move) } }
}

// Native dialog backdrops retarget pointer events to the dialog itself.
// Empty space inside the panel is not the backdrop.
export function dialogDismiss(node, options = {}) {
  let current = options
  let startedOutside = false
  const outside = event => {
    if (event.target !== node) return false
    if (node.tagName !== 'DIALOG') return true
    const rect = node.getBoundingClientRect()
    return event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom
  }
  const down = event => { startedOutside = outside(event) }
  const click = event => {
    if (!startedOutside || !outside(event)) return
    startedOutside = false
    event.stopPropagation()
    if (!current.disabled) current.onclose()
  }
  const cancel = event => {
    event.preventDefault()
    event.stopPropagation()
    if (!current.disabled) current.onclose()
  }
  node.addEventListener('pointerdown', down)
  node.addEventListener('click', click)
  node.addEventListener('cancel', cancel)
  return {
    update(options) { current = options },
    destroy() {
      node.removeEventListener('pointerdown', down)
      node.removeEventListener('click', click)
      node.removeEventListener('cancel', cancel)
    },
  }
}

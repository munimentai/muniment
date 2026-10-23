import { coverBrowser } from './browser-cover.svelte.js'

// Native dialog backdrops retarget pointer events to the dialog itself.
// Empty space inside the panel is not the backdrop.
// While the dialog is open it hides the native browser view, which would draw
// above it. A <dialog> is open while it holds `open`. `covers: false` keeps
// the view for a dialog that never sits over it.
export function dialogDismiss(node, options = {}) {
  let current = options
  let startedOutside = false
  let release = null
  const cover = () => {
    const open = current.covers !== false && (node.tagName !== 'DIALOG' || node.open)
    if (open && !release) release = coverBrowser()
    else if (!open && release) { release(); release = null }
  }
  const observer = node.tagName === 'DIALOG' ? new MutationObserver(cover) : null
  observer?.observe(node, { attributes: true, attributeFilter: ['open'] })
  cover()
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
    update(options) { current = options; cover() },
    destroy() {
      observer?.disconnect()
      release?.()
      node.removeEventListener('pointerdown', down)
      node.removeEventListener('click', click)
      node.removeEventListener('cancel', cancel)
    },
  }
}

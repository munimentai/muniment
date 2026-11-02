// The composer band's meta side gives way in priority order once the model
// name would clip below a readable width, as "GPT-6 Astra" needs: first the
// context meter drops its percent, then the permission chip its label, and
// only then the model chip shrinks to an ellipsis. Each collapsed control
// keeps its full name as its accessible name and hover label.
export const READABLE_MODEL_NAME = 72
const LEVELS = 2

function crowded(meta, floor) {
  // A hover label draws past the band, so the controls' own boxes say whether it overflows.
  const right = meta.getBoundingClientRect().right
  if ([...meta.children].some((child) => child.getBoundingClientRect().right > right + 0.5)) return true
  const label = meta.querySelector('[data-menu="model"] .label')
  if (!label || !label.getClientRects().length) return false
  return label.clientWidth + 0.5 < Math.min(label.scrollWidth, floor)
}

// Sets `data-collapse` on `meta` to the fewest steps that keep the model
// name readable, and returns that count.
export function fitComposerMeta(meta, floor = READABLE_MODEL_NAME) {
  let level = 0
  delete meta.dataset.collapse
  while (level < LEVELS && crowded(meta, floor)) meta.dataset.collapse = String(++level)
  return level
}

// A Svelte action: fits the meta side again when the band resizes or its
// controls change, at most once a frame.
export function composerPriority(meta) {
  let frame = 0
  const schedule = () => { if (!frame) frame = requestAnimationFrame(() => { frame = 0; fitComposerMeta(meta) }) }
  const sizes = typeof ResizeObserver === 'function' ? new ResizeObserver(schedule) : null
  // The meta side grows to fill the band, so its own box follows the column.
  sizes?.observe(meta)
  const changes = typeof MutationObserver === 'function' ? new MutationObserver(schedule) : null
  changes?.observe(meta, { childList: true, subtree: true, characterData: true })
  fitComposerMeta(meta)
  return {
    destroy() {
      if (frame) cancelAnimationFrame(frame)
      sizes?.disconnect()
      changes?.disconnect()
    },
  }
}

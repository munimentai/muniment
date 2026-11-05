// The composer band's meta side gives way in priority order once the model
// name would clip: first the context meter drops its percent, then the
// permission chip, the thinking chip and the machine chip their labels, and
// only then the model chip shrinks to an ellipsis. The model name is the last
// label to go: it drops to its icon only when it cannot show even a few
// letters. Each collapsed control keeps its full name as its accessible name
// and hover label.
export const COLLAPSE_ORDER = ['.context-trigger', '[data-menu="mode"]', '[data-menu="thinking"]', '[data-menu="machine"]', '[data-menu="model"]']
// The narrowest model name the chip shows before it keeps its icon alone.
export const MIN_MODEL_NAME = 40

function overflows(meta) {
  // A hover label draws past the band, so the controls' own boxes say whether it overflows.
  const right = meta.getBoundingClientRect().right
  return [...meta.children].some((child) => child.getBoundingClientRect().right > right + 0.5)
}

// The model name is readable when it shows whole, or as wide as the chip's cap allows.
function nameClipped(meta) {
  const label = meta.querySelector('[data-menu="model"] .label')
  if (!label || !label.getClientRects().length) return false
  const chip = label.closest('[data-menu="model"]')
  const cap = Number.parseFloat(getComputedStyle(chip).maxWidth)
  const room = Number.isFinite(cap) ? label.clientWidth + cap - chip.getBoundingClientRect().width : Infinity
  return label.clientWidth + 0.5 < Math.min(label.scrollWidth, room)
}

function crowded(meta, level) {
  if (overflows(meta)) return true
  const label = meta.querySelector('[data-menu="model"] .label')
  if (level < COLLAPSE_ORDER.length - 1) return nameClipped(meta)
  // Only the model chip is left: it keeps a short name with an ellipsis.
  return !!label && label.getClientRects().length > 0 && label.clientWidth < Math.min(label.scrollWidth, MIN_MODEL_NAME)
}

// Collapses the fewest controls, in `COLLAPSE_ORDER`, that keep the model name
// readable. Each collapsed control carries `data-collapsed`, and `meta`
// carries the count as `data-collapse`, which this returns.
export function fitComposerMeta(meta) {
  let level = 0
  delete meta.dataset.collapse
  for (const control of meta.querySelectorAll('[data-collapsed]')) delete control.dataset.collapsed
  while (level < COLLAPSE_ORDER.length && crowded(meta, level)) {
    for (const control of meta.querySelectorAll(COLLAPSE_ORDER[level])) control.dataset.collapsed = 'true'
    meta.dataset.collapse = String(++level)
  }
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

// Keep fades on the viewport, and reveal the end of paths without reversing text.
export function overflowFade(node, options = {}) {
  let frame
  let atEnd = true
  function measure(reset = false) {
    const max = Math.max(0, node.scrollWidth - node.clientWidth)
    if (options.end && (reset || atEnd)) node.scrollLeft = max
    atEnd = max - node.scrollLeft < 1
    node.dataset.fadeLeft = String(node.scrollLeft > 1)
    node.dataset.fadeRight = String(max - node.scrollLeft > 1)
  }
  const scroll = () => measure()
  const schedule = () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(() => measure()) }
  const resize = new ResizeObserver(schedule)
  resize.observe(node)
  // Text and tab changes can change scrollWidth without changing the viewport.
  const mutations = new MutationObserver(schedule)
  mutations.observe(node, {childList: true, subtree: true, characterData: true})
  node.addEventListener('scroll', scroll, {passive: true})
  measure(true)
  return {
    update(next) { const reset = next.text !== options.text; options = next; measure(reset) },
    destroy() { cancelAnimationFrame(frame); resize.disconnect(); mutations.disconnect(); node.removeEventListener('scroll', scroll) },
  }
}

// Marks each one-line label under a root with data-overflow, so the label's end
// fades only while its text is wider than its box.
export function overflowLabels(root, selector) {
  let frame
  const mark = (node) => {
    const over = String(node.scrollWidth - node.clientWidth > 1)
    if (node.dataset.overflow !== over) node.dataset.overflow = over
  }
  let resize = null
  const watched = new WeakSet()
  function scan() {
    for (const node of root.querySelectorAll(selector)) {
      if (!watched.has(node)) {
        watched.add(node)
        if (!resize && typeof ResizeObserver !== 'undefined') resize = new ResizeObserver((entries) => { for (const entry of entries) mark(entry.target) })
        resize?.observe(node)
      }
      mark(node)
    }
  }
  const schedule = () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(scan) }
  // Rows come and go, and a renamed row keeps its box while its text grows.
  const mutations = new MutationObserver(schedule)
  mutations.observe(root, { childList: true, subtree: true, characterData: true })
  scan()
  return { destroy() { cancelAnimationFrame(frame); resize?.disconnect(); mutations.disconnect() } }
}

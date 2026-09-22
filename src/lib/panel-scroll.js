// Shared scroll treatment for a panel body. Keep controls at each end readable
// when that end has no hidden content.
export function panelScroll(node) {
  node.setAttribute('data-panel-scroll', '')
  const update = () => {
    node.style.setProperty('--panel-fade-top', node.scrollTop > 1 ? '24px' : '0px')
    node.style.setProperty('--panel-fade-bottom', node.scrollHeight - node.clientHeight - node.scrollTop > 1 ? '24px' : '0px')
  }
  const resize = (typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(update))
  const observeChildren = () => { resize?.disconnect(); resize?.observe(node); for (const child of node.children) resize?.observe(child); update() }
  const mutations = new MutationObserver(observeChildren)
  mutations.observe(node, { childList: true, subtree: true, characterData: true })
  node.addEventListener('scroll', update, { passive: true })
  observeChildren()
  return { destroy() { resize?.disconnect(); mutations.disconnect(); node.removeEventListener('scroll', update) } }
}

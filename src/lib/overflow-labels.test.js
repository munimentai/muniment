import { afterEach, expect, it, vi } from 'vitest'
import { overflowLabels } from './overflow-labels.js'
afterEach(() => vi.unstubAllGlobals())
it('marks only the labels whose text is wider than their box, and follows resizes', () => {
  let resize
  vi.stubGlobal('ResizeObserver', class { constructor(callback) { resize = callback } observe() {} disconnect() {} })
  const root = document.createElement('div')
  const sized = (scroll, client) => {
    const node = document.createElement('span')
    node.className = 'label'
    let width = client
    Object.defineProperties(node, { scrollWidth: { value: scroll }, clientWidth: { get: () => width } })
    root.append(node)
    return { node, grow: (next) => { width = next } }
  }
  const fits = sized(80, 120)
  const long = sized(300, 120)
  const action = overflowLabels(root, '.label')
  expect(fits.node.dataset.overflow).toBe('false')
  expect(long.node.dataset.overflow).toBe('true')
  long.grow(320)
  resize([{ target: long.node }])
  expect(long.node.dataset.overflow).toBe('false')
  action.destroy()
})

import { afterEach, expect, it, vi } from 'vitest'
import { overflowFade } from './overflow-fade.js'
import { browserLabel, folderLabel } from './workspace-labels.js'
afterEach(() => vi.unstubAllGlobals())
it('shows URL context without losing subdomains, ports, queries or fragments', () => {
  expect(browserLabel('https://www.google.com/?zx=1790006248858')).toBe('google.com/?zx=1790006248858')
  expect(browserLabel('https://docs.example.co.uk:8080/a?q=b#c')).toBe('docs.example.co.uk:8080/a?q=b#c')
  expect(browserLabel('https://user:secret@example.org/')).toBe('example.org/')
  expect(folderLabel('/Users/person/work/long-folder/')).toBe('long-folder')
  expect(folderLabel('C:\\Users\\person\\work\\')).toBe('work')
  expect(folderLabel('/')).toBe('/')
})
it('starts at the path end but lets the user scroll back and responds to resizing', () => {
  let resize
  vi.stubGlobal('ResizeObserver', class { constructor(callback) { resize = callback } observe() {} disconnect() {} })
  vi.stubGlobal('requestAnimationFrame', callback => { callback(); return 1 })
  const node = document.createElement('span')
  let width = 100
  Object.defineProperties(node, {clientWidth: {get: () => width}, scrollWidth: {value: 400}})
  const action = overflowFade(node, {end: true, text: '/long/path'})
  expect(node.scrollLeft).toBe(300)
  expect(node.dataset.fadeLeft).toBe('true')
  expect(node.dataset.fadeRight).toBe('false')
  node.scrollLeft = 100
  node.dispatchEvent(new Event('scroll'))
  expect(node.scrollLeft).toBe(100)
  expect(node.dataset.fadeRight).toBe('true')
  resize()
  expect(node.scrollLeft).toBe(100)
  action.update({end: true, text: '/another/path'})
  expect(node.scrollLeft).toBe(300)
  width = 500
  resize()
  expect(node.dataset.fadeLeft).toBe('false')
  expect(node.dataset.fadeRight).toBe('false')
  action.destroy()
})

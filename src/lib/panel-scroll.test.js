import { expect, it } from 'vitest'
import { panelScroll } from './panel-scroll.js'
it('fades only edges with hidden content and removes listeners on teardown', () => {
  const node = document.createElement('div')
  Object.defineProperties(node, {scrollHeight:{value:500},clientHeight:{value:200}})
  const action = panelScroll(node)
  expect(node.style.getPropertyValue('--panel-fade-top')).toBe('0px')
  expect(node.style.getPropertyValue('--panel-fade-bottom')).toBe('24px')
  node.scrollTop = 100
  node.dispatchEvent(new Event('scroll'))
  expect(node.style.getPropertyValue('--panel-fade-top')).toBe('24px')
  node.scrollTop = 300
  node.dispatchEvent(new Event('scroll'))
  expect(node.style.getPropertyValue('--panel-fade-bottom')).toBe('0px')
  action.destroy()
  node.scrollTop = 0
  node.dispatchEvent(new Event('scroll'))
  expect(node.style.getPropertyValue('--panel-fade-top')).toBe('24px')
})

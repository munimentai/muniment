import { cleanup, render } from '@testing-library/svelte'
import { createRawSnippet } from 'svelte'
import { afterEach, expect, it, vi } from 'vitest'
import SettingsPart from './SettingsPart.svelte'

afterEach(() => { cleanup(); vi.unstubAllGlobals(); delete Element.prototype.scrollIntoView })

// A search result for About opens General with About as its part. The parts
// above it load their sections after it scrolled, so it scrolls again as the
// page grows, until the user scrolls.
it('keeps a searched part in view while the parts above it load, until the user scrolls', () => {
  const observers = []
  vi.stubGlobal('ResizeObserver', class { constructor(callback) { this.callback = callback; observers.push(this) } observe() {} disconnect() { this.gone = true } })
  const scrolled = []
  Element.prototype.scrollIntoView = function (options) { scrolled.push([this.dataset.settingsPart, options]) }
  const children = createRawSnippet(() => ({ render: () => '<p>Version</p>' }))
  const page = document.createElement('div')
  document.body.append(page)
  render(SettingsPart, { target: page, props: { id: 'about', title: 'About', focus: true, children } })
  expect(scrolled).toEqual([['about', { block: 'start' }]])
  observers[0].callback()
  expect(scrolled).toHaveLength(2)
  window.dispatchEvent(new Event('wheel'))
  expect(observers[0].gone).toBe(true)
  page.remove()
})

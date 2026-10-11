// @vitest-environment jsdom
import { readFileSync } from 'node:fs'
import { afterEach, describe, expect, it, vi } from 'vitest'

const script = readFileSync('src-tauri/src/cef_browser/observe.js', 'utf8')
const observe = () => window.eval(script)

function page(html, hidden = []) {
  document.body.innerHTML = html
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function () {
    return hidden.some(selector => this.matches(selector))
      ? { x: 0, y: 2000, left: 0, top: 2000, right: 100, bottom: 2020, width: 100, height: 20 }
      : { x: 10, y: 10, left: 10, top: 10, right: 110, bottom: 30, width: 100, height: 20 }
  })
  document.elementFromPoint = () => null
}

afterEach(() => { vi.restoreAllMocks(); delete window.__munimentNext })

describe('browser page reader', () => {
  it('numbers visible controls with their role, name, value and operations', () => {
    page(`<label for="to">Where to?</label><input id="to" value="Par">
      <button>Search</button><a href="/deals">Deals</a>
      <select aria-label="Class"><option>Economy</option><option>Business</option></select>
      <input type="checkbox" aria-label="Direct only" checked>
      <input type="password" aria-label="Password">
      <button style="visibility:hidden">Hidden</button><button disabled>Off</button>
      <p>Find flights</p><footer>Far below</footer>`, ['footer'])
    const result = observe()
    const rows = result.elements.map(({ id, ...rest }) => rest)
    expect(rows).toEqual([
      { role: 'textbox', name: 'Where to?', value: 'Par', operations: ['fill'] },
      { role: 'button', name: 'Search', operations: ['click'] },
      { role: 'link', name: 'Deals', operations: ['click'] },
      { role: 'combobox', name: 'Class', value: 'Economy', options: [{ index: '1', label: 'Economy' }, { index: '2', label: 'Business' }], operations: ['select'] },
      { role: 'checkbox', name: 'Direct only', operations: ['click'], checked: true },
      { role: 'password', name: 'Password', operations: [] },
    ])
    expect(result.text).toContain('Find flights')
    expect(result.text).not.toContain('Far below')
  })

  it('keeps an element number while the document lives', () => {
    page('<button>One</button><button>Two</button>')
    const first = observe().elements.map(e => e.id)
    document.body.insertAdjacentHTML('afterbegin', '<button>Zero</button>')
    const second = observe().elements
    expect(second.find(e => e.name === 'One').id).toBe(first[0])
    expect(second.find(e => e.name === 'Two').id).toBe(first[1])
    expect(new Set(second.map(e => e.id)).size).toBe(3)
  })

  it('leaves out a control that a dialog covers', () => {
    page('<button id="under">Under</button><div id="cover">Accept cookies</div>')
    document.elementFromPoint = () => document.getElementById('cover')
    expect(observe().elements).toEqual([])
  })
})

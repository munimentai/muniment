import { afterEach, expect, it } from 'vitest'
import { pressedElsewhere, recordPress } from './press-target.js'

const rows = () => {
  document.body.innerHTML = '<ul><li id="a"><button id="a-title">A</button></li><li id="b"><button>B</button></li></ul>'
  return { a: document.getElementById('a'), b: document.getElementById('b'), title: document.getElementById('a-title') }
}
const press = (target, init) => target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, ...init }))
const menuOn = (row, init = { clientX: 10, clientY: 10 }) => {
  let elsewhere = null
  row.addEventListener('contextmenu', (event) => { elsewhere = pressedElsewhere(event) }, { once: true })
  row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, ...init }))
  return elsewhere
}
afterEach(() => { document.body.innerHTML = ''; recordPress({ button: 0 }) })

it('names a context menu that follows a right or Control press on another row', () => {
  const { a, b, title } = rows()
  press(title, { button: 2 })
  expect(menuOn(b)).toBe(true)
  press(b, { button: 0, ctrlKey: true })
  expect(menuOn(a)).toBe(true)
})

it('passes a context menu on the row the press began on, from the keyboard, or with no press', () => {
  const { a, title } = rows()
  press(title, { button: 2 })
  expect(menuOn(a)).toBe(false)
  press(title, { button: 2 })
  expect(menuOn(document.getElementById('b'), { clientX: 0, clientY: 0 })).toBe(false)
  expect(menuOn(a)).toBe(false)
  // A primary press without Control opens no menu, so it names no row.
  press(title, { button: 0 })
  expect(menuOn(document.getElementById('b'))).toBe(false)
})

it('keeps the default off a secondary press on a sidebar row, so no word is selected and the list keeps its scroll', () => {
  document.body.innerHTML = '<aside data-panel="sidebar"><button id="row"><span id="title">Plan the week</span></button><input id="field"></aside><main><p id="text">Reply</p></main>'
  const down = (id, init) => {
    const event = new MouseEvent('mousedown', { bubbles: true, cancelable: true, ...init })
    document.getElementById(id).dispatchEvent(event)
    return event.defaultPrevented
  }
  expect(down('title', { button: 2 })).toBe(true)
  expect(down('row', { button: 0, ctrlKey: true })).toBe(true)
  // A primary press, a text field and the rest of the window keep their defaults.
  expect(down('title', { button: 0 })).toBe(false)
  expect(down('field', { button: 2 })).toBe(false)
  expect(down('text', { button: 2 })).toBe(false)
})

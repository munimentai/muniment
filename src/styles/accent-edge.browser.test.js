import { afterEach, expect, it } from 'vitest'
import './tokens.css'
import './base.css'

afterEach(() => {
  document.body.replaceChildren()
  delete document.documentElement.dataset.theme
  delete document.documentElement.dataset.frame
})

// A box as wide as the default sidebar's open row, with a rounded corner.
function edged(color = '') {
  const box = document.createElement('div')
  box.dataset.accentEdge = ''
  Object.assign(box.style, { width: '184px', height: '36px', borderRadius: '6px' })
  if (color) box.style.setProperty('--edge-color', color)
  document.body.append(box)
  return box
}

const edge = (box) => getComputedStyle(box, '::before')

it('draws a solid thin line down the left edge of a frameless window', () => {
  document.documentElement.dataset.frame = 'frameless'
  const line = edge(edged())
  expect(line.position).toBe('absolute')
  expect(line.left).toBe('0px')
  expect(line.top).toBe('0px')
  expect(line.bottom).toBe('0px')
  expect(line.width).toBe('2px')
  expect(line.backgroundImage).toBe('none')
  expect(line.pointerEvents).toBe('none')
})

it('rounds the line with the box corners in a framed window', () => {
  document.documentElement.dataset.frame = 'framed'
  const line = edge(edged())
  expect(line.borderLeftWidth).toBe('2px')
  expect(line.borderLeftStyle).toBe('solid')
  expect(line.borderTopLeftRadius).toBe('6px')
  expect(line.backgroundColor).toBe('rgba(0, 0, 0, 0)')
})

it('takes the theme accent by default and an agent color when one is set', () => {
  for (const frame of ['frameless', 'framed']) {
    document.documentElement.dataset.frame = frame
    const color = (box) => frame === 'framed' ? edge(box).borderLeftColor : edge(box).backgroundColor
    for (const theme of ['moss-light', 'moss-dark']) {
      document.documentElement.dataset.theme = theme
      const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()
      const probe = document.createElement('i')
      probe.style.color = accent
      document.body.append(probe)
      expect(color(edged())).toBe(getComputedStyle(probe).color)
      document.body.replaceChildren()
    }
    expect(color(edged('#5fb98a'))).toBe('rgb(95, 185, 138)')
  }
})

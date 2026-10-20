import { afterEach, expect, it } from 'vitest'
import './tokens.css'
import './base.css'

afterEach(() => {
  document.body.replaceChildren()
  delete document.documentElement.dataset.theme
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

it('draws a thin line down the left edge that fades toward the top and the bottom', () => {
  const line = edge(edged())
  expect(line.position).toBe('absolute')
  expect(line.left).toBe('0px')
  expect(line.top).toBe('0px')
  expect(line.bottom).toBe('0px')
  expect(line.width).toBe('2px')
  // Transparent at both ends, so the line never follows a rounded corner.
  expect(line.backgroundImage).toMatch(/^linear-gradient\(rgba\(0, 0, 0, 0\), .+, rgba\(0, 0, 0, 0\)\)$/)
  expect(line.pointerEvents).toBe('none')
})

it('takes the theme accent by default and an agent color when one is set', () => {
  for (const theme of ['moss-light', 'moss-dark']) {
    document.documentElement.dataset.theme = theme
    const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()
    const probe = document.createElement('i')
    probe.style.color = accent
    document.body.append(probe)
    expect(edge(edged()).backgroundImage).toContain(getComputedStyle(probe).color)
    document.body.replaceChildren()
  }
  expect(edge(edged('#5fb98a')).backgroundImage).toContain('rgb(95, 185, 138)')
})

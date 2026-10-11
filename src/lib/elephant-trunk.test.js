import { describe, expect, it } from 'vitest'

import { trunkPath } from './elephant-trunk.js'

const points = (d) => [...d.matchAll(/(-?\d+(?:\.\d+)?) (-?\d+(?:\.\d+)?)(?= [LAZ])/g)].map(([, x, y]) => [Number(x), Number(y)])

describe('trunkPath', () => {
  it('draws the logo trunk at rest', () => {
    const d = trunkPath()
    expect(d).toMatch(/^M427 300 /)
    expect(d).toContain('L427 449 A10 10 0 0 0 437 459 L490 459 A36 36 0 0 0 526 423 ')
    expect(d).toMatch(/ L526 300 Z$/)
    for (const [x] of points(d)) expect([427, 437, 490, 526]).toContain(x)
  })

  it('raises the curled trunk in front of the face', () => {
    const curled = points(trunkPath(1))
    expect(Math.min(...curled.map(([, y]) => y))).toBeLessThan(338)
    expect(Math.max(...curled.map(([x]) => x))).toBeGreaterThan(526)
  })

  it('sways the tip forward and back', () => {
    const tipX = (sway) => Math.max(...points(trunkPath(0, sway)).map(([x]) => x))
    expect(tipX(1)).toBeGreaterThan(tipX(0))
    expect(tipX(-1)).toBeLessThan(tipX(0) + 0.01)
  })
})

it('draws every curl with the same commands, so an animation can morph between them', () => {
  const commands = (d) => d.replace(/[\d.-]+/g, '#')
  expect(commands(trunkPath(0.5, 0.3))).toBe(commands(trunkPath()))
  expect(commands(trunkPath(1))).toBe(commands(trunkPath()))
})

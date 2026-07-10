import { describe, expect, it } from 'vitest'

import { ringFrame, ringPath, ringPhase, solidRingPath } from './mark.js'

describe('ringPath', () => {
  it('returns a closed SVG path with one segment per sample', () => {
    const path = ringPath(44)
    const coordinates = path.match(/\d+\.\d{2},\d+\.\d{2}/g)

    expect(path).toMatch(/^M\d+\.\d{2},\d+\.\d{2}( L\d+\.\d{2},\d+\.\d{2})+ Z$/)
    expect(coordinates).toHaveLength(45)
    expect(coordinates.at(-1)).toBe(coordinates[0])
  })

  it('produces deterministic reference geometry', () => {
    expect(ringPath(4)).toBe(
      'M40.50,24.00 L24.00,40.50 L7.50,24.00 L24.00,7.50 L40.50,24.00 Z',
    )
  })
})

describe('ring animation engine', () => {
  it('uses a shared epoch so instances mounted at different times stay in phase', () => {
    const epoch = 1_000
    const firstInstance = ringFrame(ringPhase(5_250, epoch))
    const laterInstance = ringFrame(ringPhase(5_250, epoch))

    expect(laterInstance).toEqual(firstInstance)
  })

  it('produces bounded breath and trace values', () => {
    const frame = ringFrame(6.25)

    expect(frame.scale).toBeGreaterThanOrEqual(0.968)
    expect(frame.scale).toBeLessThanOrEqual(1.032)
    expect(frame.strokeScale).toBeGreaterThanOrEqual(0.82)
    expect(frame.strokeScale).toBeLessThanOrEqual(1.18)
    expect(frame.trace).toBeGreaterThanOrEqual(0)
    expect(frame.trace).toBeLessThanOrEqual(1)
  })

  it('builds the closed even-odd geometry for the small-size reduction', () => {
    expect(solidRingPath(22)).toMatch(/^M.+ Z M.+ Z$/)
  })
})

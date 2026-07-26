import { describe, expect, it } from 'vitest'

import { MILLED_RING_PATH, ringPath, solidMilledRingPath } from './mark.js'

describe('ringPath', () => {
  it('returns the canonical vendored geometry', () => {
    expect(ringPath()).toBe(MILLED_RING_PATH)
    expect(ringPath()).toMatch(/^M40\.50,24\.00 .+ L40\.50,24\.00 Z$/)
  })

  it('preserves all owner-supplied vertices', () => {
    expect(ringPath().match(/\d+\.\d{2},\d+\.\d{2}/g)).toHaveLength(221)
  })
})

describe('solidMilledRingPath', () => {
  const coordinates = (subpath) => [...subpath.matchAll(/[ML](-?\d+\.\d+),(-?\d+\.\d+)/g)]
    .map(([, x, y]) => [Number(x), Number(y)])

  it('generates two deterministic closed sampled edges', () => {
    const first = solidMilledRingPath()
    const second = solidMilledRingPath()
    const subpaths = first.split(' Z')

    expect(first).toBe(second)
    expect(first.match(/\bM/g)).toHaveLength(2)
    expect(first.match(/\bZ/g)).toHaveLength(2)
    expect(subpaths.slice(0, 2).map((subpath) => coordinates(subpath).length)).toEqual([264, 264])
  })

  it('keeps both edges within their radial bounds', () => {
    const edges = solidMilledRingPath().split(' Z').slice(0, 2).map(coordinates)
    const radii = edges.map((points) => points.map(([x, y]) => Math.hypot(x - 24, y - 24)))

    expect(Math.min(...radii[0])).toBeCloseTo(17.4, 2)
    expect(Math.max(...radii[0])).toBeCloseTo(20.6, 2)
    expect(Math.min(...radii[1])).toBeCloseTo(12.4, 2)
    expect(Math.max(...radii[1])).toBeCloseTo(15.6, 2)
  })

  it('preserves all 22 milling teeth on each edge', () => {
    const edges = solidMilledRingPath().split(' Z').slice(0, 2).map(coordinates)
    const peakCount = (points) => {
      const radii = points.map(([x, y]) => Math.hypot(x - 24, y - 24))
      return radii.filter((radius, index) => (
        radius > radii[(index + radii.length - 1) % radii.length]
        && radius > radii[(index + 1) % radii.length]
      )).length
    }

    expect(edges.map(peakCount)).toEqual([22, 22])
  })
})

import { describe, expect, it } from 'vitest'

import { MILLED_RING_PATH, SEAL_BAND_WIDTH, milledRingPoints, ringPath, sealBandPath, sealGraphPoints, sealTracePath, solidMilledRingPath } from './mark.js'

describe('ringPath', () => {
  it('returns the canonical vendored geometry', () => {
    expect(ringPath()).toBe(MILLED_RING_PATH)
    expect(ringPath()).toMatch(/^M44\.20,24\.00 .+ L44\.20,24\.00 Z$/)
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

    expect(Math.min(...radii[0])).toBeCloseTo(21.55, 2)
    expect(Math.max(...radii[0])).toBeCloseTo(23.85, 2)
    expect(Math.min(...radii[1])).toBeCloseTo(16.55, 2)
    expect(Math.max(...radii[1])).toBeCloseTo(18.85, 2)
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

describe('sealBandPath', () => {
  const coordinates = (subpath) => [...subpath.matchAll(/[ML](-?\d+\.\d+),(-?\d+\.\d+)/g)]
    .map(([, x, y]) => [Number(x), Number(y)])
  const radii = (points) => points.map(([x, y]) => Math.hypot(x - 24, y - 24))

  it('draws the seal as two closed edges of 22 teeth, five vertices each', () => {
    const edges = sealBandPath().split(' Z').slice(0, 2).map(coordinates)
    expect(sealBandPath().match(/\bZ/g)).toHaveLength(2)
    expect(edges.map((edge) => edge.length)).toEqual([110, 110])
    expect(sealBandPath()).toBe(sealBandPath(1, 1, SEAL_BAND_WIDTH))
  })

  it('keeps the rest band between the measured seal radii', () => {
    const [outer, inner] = sealBandPath().split(' Z').slice(0, 2).map(coordinates).map(radii)
    expect(Math.min(...outer)).toBeCloseTo(20.967, 2)
    expect(Math.max(...outer)).toBeCloseTo(23.906, 2)
    expect(Math.min(...inner)).toBeCloseTo(17.170, 2)
    expect(Math.max(...inner)).toBeCloseTo(19.945, 2)
  })

  it('flexes scale, milling depth and band width together', () => {
    const rest = sealBandPath().split(' Z').slice(0, 2).map(coordinates).map(radii)
    const swell = sealBandPath(1.6, 1.032, SEAL_BAND_WIDTH * 1.18).split(' Z').slice(0, 2).map(coordinates).map(radii)
    const depth = (edge) => Math.max(...edge) - Math.min(...edge)
    expect(depth(swell[0])).toBeGreaterThan(depth(rest[0]))
    expect(Math.max(...swell[0])).toBeGreaterThan(Math.max(...rest[0]))
    expect(Math.min(...swell[1])).toBeLessThan(Math.min(...rest[1]))
    const wide = sealBandPath(1, 1, 6).split(' Z').slice(0, 2).map(coordinates).map(radii)
    expect(Math.max(...wide[0]) - Math.max(...rest[0])).toBeCloseTo((6 - SEAL_BAND_WIDTH) / 2, 2)
  })

  it('runs the trace along the band centreline', () => {
    const trace = radii(coordinates(sealTracePath()))
    expect(trace).toHaveLength(44)
    expect(Math.min(...trace)).toBeCloseTo((20.967 + 17.170) / 2, 2)
    expect(Math.max(...trace)).toBeCloseTo((23.906 + 19.945) / 2, 2)
  })
})

describe('milledRingPoints', () => {
  it('reads the ring\'s own vertices and drops the closing repeat', () => {
    const points = milledRingPoints()
    expect(points.length).toBeGreaterThan(200)
    expect(points[0]).not.toEqual(points.at(-1))
    expect(MILLED_RING_PATH.startsWith(`M${points[0][0].toFixed(2)},${points[0][1].toFixed(2)}`)).toBe(true)
  })

  it('keeps every point on the 22-tooth wave', () => {
    for (const [x, y] of milledRingPoints()) {
      const radius = Math.hypot(x - 24, y - 24)
      expect(radius).toBeGreaterThan(18.5)
      expect(radius).toBeLessThan(22)
    }
  })

  it('samples every nth point', () => {
    const every = milledRingPoints()
    const third = milledRingPoints(3)
    expect(third).toHaveLength(Math.ceil(every.length / 3))
    expect(third[1]).toEqual(every[3])
  })
})

describe('sealGraphPoints', () => {
  const radius = ([x, y]) => Math.hypot(x - 24, y - 24)
  const bearing = ([x, y]) => Math.atan2(y - 24, x - 24) * 180 / Math.PI

  it('names every vertex of the 22 teeth, valley first from the first valley axis', () => {
    const points = sealGraphPoints()
    expect(points).toHaveLength(110)
    expect(bearing(points[0])).toBeCloseTo(12.27, 1)
    expect(radius(points[0])).toBeLessThan(radius(points[2]))
  })

  it('pushes the wave 1.9 times about the mean and holds it between 15.5 and 23.6', () => {
    const radii = sealGraphPoints().map(radius)
    expect(Math.min(...radii)).toBeCloseTo(19.34, 1)
    expect(Math.max(...radii)).toBeCloseTo(23.6, 2)
    for (const r of radii) {
      expect(r).toBeGreaterThanOrEqual(15.5)
      expect(r).toBeLessThanOrEqual(23.6 + 1e-9)
    }
  })

  it('is unpushed at 1 and so sits on the seal band', () => {
    const radii = sealGraphPoints(1, 0, 48).map(radius)
    expect(Math.min(...radii)).toBeCloseTo(20.967, 2)
    expect(Math.max(...radii)).toBeCloseTo(23.906, 2)
  })
})

import { afterEach, describe, expect, it, vi } from 'vitest'

import { REST_POSE, paint, step, subscribe } from './ring-motion.js'

afterEach(() => vi.restoreAllMocks())

const fakePath = () => {
  const attributes = {}
  return { attributes, style: {}, setAttribute: (name, value) => { attributes[name] = value }, getTotalLength: () => 200 }
}

describe('ring motion', () => {
  it('steps a pose whose scale flexes a few percent and whose angle stays in a turn', () => {
    let now = 0
    const poses = []
    for (let i = 0; i < 60 * 30; i += 1) poses.push(step(now += 1000 / 60))
    for (const pose of poses) {
      expect(Math.abs(pose.scale - 1)).toBeLessThanOrEqual(0.032 + 1e-9)
      expect(Math.abs(pose.ampMul - 1)).toBeLessThanOrEqual(0.6 + 1e-9)
      expect(pose.angle).toBeGreaterThanOrEqual(-360)
      expect(pose.angle).toBeLessThan(360)
    }
    expect(new Set(poses.map((pose) => pose.scale.toFixed(4))).size).toBeGreaterThan(10)
    expect(poses.some((pose) => pose.trace !== null)).toBe(true)
  })

  it('paints the rest pose as the unrotated seal with the accent hidden', () => {
    const group = fakePath(); const body = fakePath(); const accent = fakePath()
    paint(REST_POSE, { group, body, accent, width: 5.8 })
    expect(group.attributes.transform).toBe('rotate(0.00 24 24)')
    expect(body.attributes.d).toMatch(/^M.+ Z M.+ Z$/)
    expect(accent.style.opacity).toBe(0)
    expect(accent.attributes.d).toBeUndefined()
  })

  it('paints a trace as a dash segment on the centreline in signal', () => {
    const group = fakePath(); const body = fakePath(); const accent = fakePath()
    paint({ ...REST_POSE, angle: 90, trace: 0.5 }, { group, body, accent, width: 5.8 })
    expect(group.attributes.transform).toBe('rotate(90.00 24 24)')
    expect(accent.attributes['stroke-width']).toBe((5.8 * 1.15).toFixed(2))
    expect(accent.style.strokeDasharray).toBe('32 200')
    expect(accent.style.opacity).toBeCloseTo(0.9, 6)
  })

  it('never starts the clock under reduced motion', () => {
    vi.stubGlobal('matchMedia', () => ({ matches: true }))
    const raf = vi.fn()
    vi.stubGlobal('requestAnimationFrame', raf)
    const unsubscribe = subscribe(() => {})
    expect(raf).not.toHaveBeenCalled()
    unsubscribe()
    vi.unstubAllGlobals()
  })
})

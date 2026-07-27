// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'

import { thinkingSettle } from './thinking-transition.js'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('thinkingSettle', () => {
  it('finishes immediately when the user prefers reduced motion', () => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: true })))

    expect(thinkingSettle().duration).toBe(0)
  })

  it('uses the default duration without a reduced-motion preference', () => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false })))

    expect(thinkingSettle().duration).toBe(180)
  })

  it('interpolates opacity, position, and scale', () => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false })))
    const { css } = thinkingSettle()

    expect(css(0)).toBe('opacity: 0; transform: translateY(-2px) scale(0.96)')
    expect(css(0.5)).toBe('opacity: 0.5; transform: translateY(-1px) scale(0.98)')
    expect(css(1)).toBe('opacity: 1; transform: translateY(0px) scale(1)')
  })
})

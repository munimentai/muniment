import { afterEach, describe, expect, it } from 'vitest'

import { DEFAULT_FRAME, FRAME_OPTIONS, FRAME_STORAGE_KEY, applyFrame, commitFrame, parseFrame, readStoredFrame } from './frame-state.js'

afterEach(() => {
  localStorage.clear()
  delete document.documentElement.dataset.frame
})

describe('frame state', () => {
  it('offers Framed and Frameless and defaults to Framed', () => {
    expect(FRAME_OPTIONS.map(([, label]) => label)).toEqual(['Framed', 'Frameless'])
    expect(DEFAULT_FRAME).toBe('framed')
    expect(FRAME_STORAGE_KEY).toBe('muniment.frame')
  })

  it('restores the default from a missing or unknown value', () => {
    expect(parseFrame(null)).toBe('framed')
    expect(parseFrame('')).toBe('framed')
    expect(parseFrame('Frameless')).toBe('framed')
    expect(parseFrame('frameless')).toBe('frameless')
    expect(readStoredFrame()).toBe('framed')
    localStorage.setItem('muniment.frame', 'frameless')
    expect(readStoredFrame()).toBe('frameless')
  })

  it('stores a pick and applies it to the root at once', () => {
    expect(commitFrame('frameless')).toBe('frameless')
    expect(localStorage.getItem('muniment.frame')).toBe('frameless')
    expect(document.documentElement.dataset.frame).toBe('frameless')
    applyFrame(document.documentElement, 'invalid')
    expect(document.documentElement.dataset.frame).toBe('framed')
  })
})

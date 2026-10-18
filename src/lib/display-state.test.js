import { afterEach, describe, expect, it, vi } from 'vitest'
import { DISPLAY_EVENT, DISPLAY_STORAGE_KEY, applyDisplay, commitDisplay, currentZoom, motionReduced, parseDisplay, readStoredDisplay, toWindowPixels, watchMotion, zoomLabel } from './display-state.js'

const reduceMedia = (matches) => { window.matchMedia = vi.fn((query) => ({ matches: query.includes('reduced-motion') && matches, media: query, addEventListener: vi.fn(), removeEventListener: vi.fn() })) }

afterEach(() => {
  vi.restoreAllMocks()
  delete window.matchMedia
  localStorage.clear()
  delete document.documentElement.dataset.density
  delete document.documentElement.dataset.motion
  applyDisplay(document.documentElement, {}, { setZoom: () => {} })
})

describe('display state', () => {
  it('restores the defaults from missing or malformed values', () => {
    expect(parseDisplay(null)).toEqual({ zoom: 1, density: 'comfortable', motion: 'system' })
    expect(parseDisplay('{')).toEqual({ zoom: 1, density: 'comfortable', motion: 'system' })
    expect(parseDisplay('{"zoom":3,"density":"tight","motion":"off"}')).toEqual({ zoom: 1, density: 'comfortable', motion: 'system' })
    expect(parseDisplay({ zoom: 1.25, density: 'compact', motion: 'reduce' })).toEqual({ zoom: 1.25, density: 'compact', motion: 'reduce' })
    expect(zoomLabel(1.25)).toBe('125%')
  })

  it('follows the OS under System and lets the pick override it either way', () => {
    reduceMedia(true)
    expect(motionReduced({ motion: 'system' })).toBe(true)
    expect(motionReduced({ motion: 'full' })).toBe(false)
    reduceMedia(false)
    expect(motionReduced({ motion: 'system' })).toBe(false)
    expect(motionReduced({ motion: 'reduce' })).toBe(true)
  })

  it('marks the root with the density and the motion that apply, and zooms the webview once per change', () => {
    reduceMedia(false)
    const root = document.documentElement
    const webview = { setZoom: vi.fn() }
    applyDisplay(root, { zoom: 1.25, density: 'compact', motion: 'reduce' }, webview)
    expect(root.dataset).toMatchObject({ density: 'compact', motion: 'reduce' })
    expect(webview.setZoom).toHaveBeenCalledWith(1.25)
    expect(currentZoom()).toBe(1.25)
    expect(toWindowPixels(100)).toBe(125)
    applyDisplay(root, { zoom: 1.25, density: 'comfortable', motion: 'system' }, webview)
    expect(webview.setZoom).toHaveBeenCalledTimes(1)
    expect(root.dataset.density).toBeUndefined()
    expect(root.dataset.motion).toBeUndefined()
  })

  it('stores and announces a change, and a watcher hears it', () => {
    reduceMedia(false)
    const heard = vi.fn()
    const stop = watchMotion(heard)
    const announced = vi.fn()
    window.addEventListener(DISPLAY_EVENT, announced)
    const next = commitDisplay({ zoom: 1, density: 'compact', motion: 'reduce' })
    expect(JSON.parse(localStorage.getItem(DISPLAY_STORAGE_KEY))).toEqual(next)
    expect(readStoredDisplay()).toEqual(next)
    expect(announced.mock.calls[0][0].detail).toEqual(next)
    expect(heard).toHaveBeenCalledTimes(1)
    stop()
    commitDisplay({ motion: 'full' })
    expect(heard).toHaveBeenCalledTimes(1)
    window.removeEventListener(DISPLAY_EVENT, announced)
  })
})

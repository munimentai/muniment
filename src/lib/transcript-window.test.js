import { describe, expect, it } from 'vitest'
import { WINDOW_EDGE, WINDOW_MAX, WINDOW_SIZE, WINDOW_STEP, createTranscriptWindow, earlier, freeze, later, tailWindow, windowBounds, windowItems } from './transcript-window.js'

const items = (count) => Array.from({ length: count }, (_, index) => index)

describe('transcript window', () => {
  it('renders a short thread whole and a long one by its newest items', () => {
    const short = items(12)
    expect(windowItems(short, tailWindow())).toBe(short)
    expect(windowItems(items(50_000), tailWindow())).toEqual(items(50_000).slice(-WINDOW_SIZE))
    expect(windowBounds(50_000)).toEqual({ start: 50_000 - WINDOW_SIZE, end: 50_000 })
  })

  it('grows toward older items and never holds more than the cap', () => {
    let view = tailWindow()
    for (let step = 0; step < 20; step += 1) {
      view = earlier(5_000, view)
      const { start, end } = windowBounds(5_000, view)
      expect(end - start).toBeLessThanOrEqual(WINDOW_MAX)
    }
    expect(windowBounds(5_000, view)).toEqual({ start: 5_000 - WINDOW_SIZE - 20 * WINDOW_STEP, end: 5_000 - WINDOW_SIZE - 20 * WINDOW_STEP + WINDOW_MAX })
    // The far end returns to following once newer items reach it.
    for (let step = 0; step < 40; step += 1) view = later(5_000, view)
    expect(view.end).toBeNull()
    expect(windowBounds(5_000, view).end).toBe(5_000)
  })

  it('stays put at the first item and follows the tail while it follows', () => {
    const top = { end: WINDOW_MAX, size: WINDOW_MAX }
    expect(earlier(5_000, top)).toBe(top)
    const tail = tailWindow()
    expect(later(5_000, tail)).toBe(tail)
    expect(windowBounds(5_001, tail)).toEqual({ start: 5_001 - WINDOW_SIZE, end: 5_001 })
  })

  it('stops following when the reader leaves the end', () => {
    const view = freeze(300, tailWindow())
    expect(view).toEqual({ end: 300, size: WINDOW_SIZE })
    expect(windowBounds(302, view)).toEqual({ start: 300 - WINDOW_SIZE, end: 300 })
    expect(freeze(302, view)).toBe(view)
  })

  it('keeps the anchor item in place when older items arrive above it', async () => {
    let view = tailWindow()
    const anchor = { top: 900, isConnected: true, classList: { contains: () => false }, getBoundingClientRect() { return { top: this.top } } }
    const edge = { classList: { contains: () => true }, nextElementSibling: anchor }
    const thread = { scrollTop: 100, scrollHeight: 4_000, clientHeight: 700, querySelectorAll: () => [edge, { previousElementSibling: anchor }] }
    const scrolls = []
    const controller = createTranscriptWindow({
      readThread: () => thread,
      readLength: () => 5_000,
      readPinned: () => false,
      readWindow: () => view,
      onWindow: (next) => { view = next },
      onScrollTop: (top) => scrolls.push(top),
      // The new items above push the anchor down by their height.
      tick: async () => { anchor.top += 2_400 },
    })
    expect(thread.scrollTop).toBeLessThan(WINDOW_EDGE)
    await controller.scrolled()
    expect(windowBounds(5_000, view)).toEqual({ start: 5_000 - WINDOW_SIZE - WINDOW_STEP, end: 5_000 })
    expect(thread.scrollTop).toBe(2_500)
    expect(scrolls).toEqual([2_500])
  })
})

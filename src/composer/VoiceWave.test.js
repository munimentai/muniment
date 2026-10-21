import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { flushSync } from 'svelte'
import VoiceWave from './VoiceWave.svelte'
import { recordingContext } from './voice-wave-fixtures.js'

afterEach(() => {
  cleanup()
  document.documentElement.style.removeProperty('--accent')
})

// A fake frame clock, level source and canvas context for the wave.
function setup(props = {}) {
  const frames = []
  let now = 0
  let listener
  const unlisten = vi.fn()
  const ctx = recordingContext()
  const composer = document.createElement('div')
  composer.style.borderTop = '1px solid rgb(47, 51, 50)'
  composer.style.borderRadius = '18px'
  document.body.append(composer)
  const view = render(VoiceWave, {
    target: composer,
    props: {
      active: false,
      listen: async (event, callback) => { expect(event).toBe('dictation-event'); listener = callback; return unlisten },
      requestFrame: (run) => { frames.push(run); return frames.length },
      cancelFrame: vi.fn(),
      context: () => ctx,
      random: () => 0.5,
      ...props,
    },
  })
  // The layout jsdom lacks: the canvas spans the composer's 760px top edge.
  const canvas = view.getByTestId('voice-wave-line')
  Object.defineProperty(canvas, 'clientWidth', { value: 760 })
  Object.defineProperty(canvas, 'clientHeight', { value: 48 })
  return {
    view,
    ctx,
    unlisten,
    pending: () => frames.length,
    level: (level) => listener({ payload: { type: 'level', level } }),
    // Runs the queued frames for `seconds` at 60 frames a second, or until the loop stops.
    advance(seconds) {
      for (let i = 0; i < Math.round(seconds * 60) && frames.length; i += 1) {
        now += 1000 / 60
        frames.shift()(now)
      }
    },
  }
}

it('runs no frame loop while dictation is idle', async () => {
  const wave = setup()
  await Promise.resolve()
  expect(wave.pending()).toBe(0)
  expect(wave.view.getByTestId('voice-wave-line')).toHaveAttribute('aria-hidden', 'true')
})

it('follows the voice levels while dictation listens, then fades out and stops its loop', async () => {
  document.documentElement.style.setProperty('--accent', '#A894F2')
  const wave = setup()
  await Promise.resolve()
  await wave.view.rerender({ active: true })
  flushSync()
  expect(wave.pending()).toBe(1)
  wave.level(0.9)
  wave.advance(1)
  const loud = wave.ctx.strokes.at(-1)
  expect(loud.shadowColor).toBe('#A894F2')
  expect(loud.shadowBlur).toBeGreaterThan(8)
  // The middle warms toward the warm color while the voice is loud.
  const middle = wave.ctx.gradients.at(-1).stops[2][1]
  expect(middle).not.toBe('rgb(168, 148, 242)')
  expect(wave.ctx.gradients.at(-2).stops[0][1]).toBe('rgb(47, 51, 50)')
  expect(wave.pending()).toBe(1)

  await wave.view.rerender({ active: false })
  const drawn = wave.ctx.strokes.length
  wave.advance(0.2)
  expect(wave.ctx.strokes.length).toBeGreaterThan(drawn)
  expect(wave.ctx.strokes.at(-1).shadowBlur).toBeLessThan(loud.shadowBlur)
  wave.advance(10)
  expect(wave.pending()).toBe(0)
  const settled = wave.ctx.strokes.length
  const cleared = wave.ctx.cleared
  wave.advance(1)
  expect(wave.ctx.strokes.length).toBe(settled)
  expect(wave.ctx.cleared).toBe(cleared)
})

it('ignores events that carry no level and starts over on the next dictation', async () => {
  const wave = setup()
  await Promise.resolve()
  await wave.view.rerender({ active: true })
  wave.level(1)
  wave.level(Number.NaN)
  wave.advance(0.5)
  const first = wave.ctx.strokes.at(-1).shadowBlur
  expect(first).toBeGreaterThan(8)
  await wave.view.rerender({ active: false })
  wave.advance(10)
  expect(wave.pending()).toBe(0)
  // A new dictation waits for its own levels instead of the last one.
  await wave.view.rerender({ active: true })
  wave.advance(0.5)
  expect(wave.ctx.strokes.at(-1).shadowBlur).toBeLessThan(4.1)
})

it('stops listening and cancels its frame when it unmounts', async () => {
  const cancelFrame = vi.fn()
  const wave = setup({ cancelFrame })
  await Promise.resolve()
  await wave.view.rerender({ active: true })
  wave.view.unmount()
  expect(wave.unlisten).toHaveBeenCalledOnce()
  expect(cancelFrame).toHaveBeenCalledOnce()
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { handsFreeActivationDelay } from './dictation-state.js'
import { createVoiceGesture } from './voice-gesture.js'

function setup() {
  let requested = false
  let status = { state: 'idle' }
  let signedIn = true
  let activeRun = false
  let dictationBusy = false
  const start = vi.fn(() => {
    requested = true
    status = { state: 'starting' }
  })
  const stop = vi.fn(() => {
    requested = false
    status = { state: 'stopped' }
  })
  const gesture = createVoiceGesture({
    start,
    stop,
    readRequested: () => requested,
    readStatus: () => status,
    busy: () => dictationBusy,
    signedIn: () => signedIn,
    hasActiveRun: () => activeRun,
  })
  return {
    gesture,
    start,
    stop,
    setActiveRun(next) { activeRun = next },
    setBusy(next) { dictationBusy = next },
    setSignedIn(next) { signedIn = next },
  }
}

function pointerEvent(type, pointerId = 1) {
  return {
    type,
    pointerId,
    button: 0,
    preventDefault: vi.fn(),
    currentTarget: { setPointerCapture: vi.fn() },
  }
}

function keyEvent(type, key = 'Enter') {
  return { type, key, repeat: false, preventDefault: vi.fn() }
}

describe('voice gesture', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it.each([
    ['pointer', (gesture) => gesture.pointerDown(pointerEvent('pointerdown')), (gesture) => gesture.pointerEnd(pointerEvent('pointerup'))],
    ['keyboard', (gesture) => gesture.keyDown(keyEvent('keydown')), (gesture) => gesture.keyUp(keyEvent('keyup'))],
    ['click', (gesture) => gesture.click(), () => {}],
    ['global', (gesture) => gesture.globalShortcut({ state: 'Pressed' }), (gesture) => gesture.globalShortcut({ state: 'Released' })],
  ])('promotes a rapid second %s activation to hands-free and stops on the next activation', (_, activate, release) => {
    const context = setup()
    activate(context.gesture)
    release(context.gesture)
    vi.advanceTimersByTime(handsFreeActivationDelay - 1)
    activate(context.gesture)
    release(context.gesture)

    expect(context.start).toHaveBeenCalledTimes(1)
    expect(context.stop).not.toHaveBeenCalled()

    activate(context.gesture)
    release(context.gesture)
    expect(context.stop).toHaveBeenCalledTimes(1)
  })

  it('stops when release occurs at the hands-free threshold', () => {
    const context = setup()
    context.gesture.pointerDown(pointerEvent('pointerdown'))
    vi.advanceTimersByTime(handsFreeActivationDelay)
    context.gesture.pointerEnd(pointerEvent('pointerup'))

    expect(context.stop).toHaveBeenCalledOnce()
    expect(context.stop).toHaveBeenCalledWith()
  })

  it('restores the cancelled capture on pointercancel', () => {
    const context = setup()
    context.gesture.pointerDown(pointerEvent('pointerdown'))
    context.gesture.pointerEnd(pointerEvent('pointercancel'))

    expect(context.stop).toHaveBeenCalledWith(true)
  })

  it.each([
    ['pointer', (gesture) => {
      gesture.pointerDown(pointerEvent('pointerdown'))
      vi.advanceTimersByTime(handsFreeActivationDelay)
      gesture.pointerEnd(pointerEvent('pointerup'))
    }],
    ['keyboard', (gesture) => {
      gesture.keyDown(keyEvent('keydown'))
      vi.advanceTimersByTime(handsFreeActivationDelay)
      gesture.keyUp(keyEvent('keyup'))
    }],
  ])('suppresses exactly one click following a %s gesture', (_, performGesture) => {
    const context = setup()
    performGesture(context.gesture)
    expect(context.stop).toHaveBeenCalledTimes(1)

    context.gesture.click()
    expect(context.start).toHaveBeenCalledTimes(1)
    expect(context.stop).toHaveBeenCalledTimes(1)

    context.gesture.click()
    expect(context.start).toHaveBeenCalledTimes(2)
  })

  it('ignores global Pressed while a run is active or dictation is busy', () => {
    const context = setup()
    context.setActiveRun(true)
    context.gesture.globalShortcut({ state: 'Pressed' })
    context.setActiveRun(false)
    context.setBusy(true)
    context.gesture.globalShortcut({ state: 'Pressed' })

    expect(context.start).not.toHaveBeenCalled()
  })

  it('pairs global Released only with its own accepted Pressed', () => {
    const context = setup()
    context.gesture.globalShortcut({ state: 'Released' })
    expect(context.stop).not.toHaveBeenCalled()

    context.gesture.globalShortcut({ state: 'Pressed' })
    context.gesture.globalShortcut({ state: 'Pressed' })
    context.gesture.globalShortcut({ state: 'Released' })
    vi.advanceTimersByTime(handsFreeActivationDelay)

    expect(context.start).toHaveBeenCalledOnce()
    expect(context.stop).toHaveBeenCalledOnce()
  })

  it('clears pending gesture timers during cleanup', () => {
    const context = setup()
    context.gesture.click()
    context.gesture.cleanup()
    vi.advanceTimersByTime(handsFreeActivationDelay)

    expect(context.stop).not.toHaveBeenCalled()
  })
})

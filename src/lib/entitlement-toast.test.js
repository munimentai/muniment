import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createEntitlementToast } from './entitlement-toast.js'

function setup() {
  let listener
  const onVisible = vi.fn()
  const unlisten = vi.fn()
  const controller = createEntitlementToast({
    listen: vi.fn(async (_, callback) => {
      listener = callback
      return unlisten
    }),
    setTimer: setTimeout,
    clearTimer: clearTimeout,
    onVisible,
  })
  return { controller, onVisible, unlisten, event: () => listener({ payload: { snapshot_version: 3 } }) }
}

describe('entitlement toast controller', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('records a rejected listener registration', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    const controller = createEntitlementToast({
      listen: vi.fn().mockRejectedValue(new Error('registration failed')),
      onVisible: vi.fn(),
    })

    await expect(controller.start()).resolves.toBeUndefined()

    expect(error).toHaveBeenCalledOnce()
    expect(error).toHaveBeenCalledWith('Entitlement listener registration failed.')
  })

  it('shows on an entitlement change and auto-dismisses after five seconds', async () => {
    const context = setup()
    await context.controller.start()

    context.event()
    expect(context.onVisible).toHaveBeenLastCalledWith(true)
    await vi.advanceTimersByTimeAsync(4999)
    expect(context.onVisible).not.toHaveBeenCalledWith(false)
    await vi.advanceTimersByTimeAsync(1)
    expect(context.onVisible).toHaveBeenLastCalledWith(false)
  })

  it('restarts the dismiss timer for a repeated event without stacking visibility', async () => {
    const context = setup()
    await context.controller.start()

    context.event()
    await vi.advanceTimersByTimeAsync(4000)
    context.event()
    await vi.advanceTimersByTimeAsync(4999)
    expect(context.onVisible).toHaveBeenLastCalledWith(true)
    await vi.advanceTimersByTimeAsync(1)
    expect(context.onVisible).toHaveBeenLastCalledWith(false)
  })

  it('clears the toast and unsubscribes during cleanup', async () => {
    const context = setup()
    await context.controller.start()
    context.event()

    context.controller.cleanup()
    expect(context.onVisible).toHaveBeenLastCalledWith(false)
    expect(context.unlisten).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(5000)
    expect(context.onVisible).toHaveBeenCalledTimes(2)
  })
})

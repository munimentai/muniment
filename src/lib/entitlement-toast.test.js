import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createComposerNotices } from './composer-notices.js'
import { ENTITLEMENT_NOTICE, createEntitlementToast } from './entitlement-toast.js'

function setup() {
  let listener
  const notices = createComposerNotices()
  notices.attach()
  const unlisten = vi.fn()
  const controller = createEntitlementToast({
    listen: vi.fn(async (_, callback) => {
      listener = callback
      return unlisten
    }),
    notices,
  })
  return { controller, notices, unlisten, event: () => listener({ payload: { snapshot_version: 3 } }) }
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
      notices: createComposerNotices(),
    })

    await expect(controller.start()).resolves.toBeUndefined()

    expect(error).toHaveBeenCalledOnce()
    expect(error).toHaveBeenCalledWith('Entitlement listener registration failed.')
  })

  it('shows a composer notice on an entitlement change that leaves after six seconds', async () => {
    const context = setup()
    await context.controller.start()

    context.event()
    expect(context.notices.current()).toMatchObject({ id: 'entitlement', text: ENTITLEMENT_NOTICE })
    await vi.advanceTimersByTimeAsync(5999)
    expect(context.notices.current()).not.toBeNull()
    await vi.advanceTimersByTimeAsync(1)
    expect(context.notices.current()).toBeNull()
  })

  it('restarts the clock for a repeated event and shows again after it left', async () => {
    const context = setup()
    await context.controller.start()

    context.event()
    await vi.advanceTimersByTimeAsync(4000)
    context.event()
    await vi.advanceTimersByTimeAsync(5999)
    expect(context.notices.current()?.id).toBe('entitlement')
    await vi.advanceTimersByTimeAsync(1)
    expect(context.notices.current()).toBeNull()

    context.event()
    expect(context.notices.current()?.id).toBe('entitlement')
  })

  it('clears the notice and unsubscribes during cleanup', async () => {
    const context = setup()
    await context.controller.start()
    context.event()

    context.controller.cleanup()
    expect(context.notices.current()).toBeNull()
    expect(context.unlisten).toHaveBeenCalledOnce()
    context.event()
    expect(context.notices.current()).toBeNull()
  })
})

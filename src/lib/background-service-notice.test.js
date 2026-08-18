import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createBackgroundServiceNotice } from './background-service-notice.js'

const reachable = { connected: true, chat_events_connected: true, supervisor_running: true }
const chatEventsDropped = { connected: true, chat_events_connected: false, supervisor_running: true }
const disconnected = { connected: false, supervisor_running: true }

function setup() {
  const onVisible = vi.fn()
  const notice = createBackgroundServiceNotice({
    setTimer: setTimeout,
    clearTimer: clearTimeout,
    onVisible,
  })
  return { notice, onVisible }
}

describe('background service notice', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('shows at once when the first status reports the service unreachable', () => {
    const context = setup()

    context.notice.update(chatEventsDropped)

    expect(context.onVisible).toHaveBeenCalledExactlyOnceWith(true)
  })

  it('ignores a status the window has not read, so the first real status still shows at once', () => {
    const context = setup()

    context.notice.update(null)
    context.notice.update(undefined)
    context.notice.update(disconnected)

    expect(context.onVisible).toHaveBeenCalledExactlyOnceWith(true)
  })

  it('stays hidden while the supervisor does not run', async () => {
    const context = setup()

    context.notice.update({ connected: false, supervisor_running: false })
    await vi.advanceTimersByTimeAsync(5_000)

    expect(context.onVisible).not.toHaveBeenCalled()
  })

  it('waits two seconds before it shows a drop that follows a reachable status', async () => {
    const context = setup()
    context.notice.update(reachable)

    context.notice.update(chatEventsDropped)
    await vi.advanceTimersByTimeAsync(1_999)
    expect(context.onVisible).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(context.onVisible).toHaveBeenCalledExactlyOnceWith(true)
  })

  it('cancels the dwell when the service reports itself reachable again', async () => {
    const context = setup()
    context.notice.update(reachable)
    context.notice.update(chatEventsDropped)

    await vi.advanceTimersByTimeAsync(250)
    context.notice.update(reachable)
    await vi.advanceTimersByTimeAsync(5_000)

    expect(context.onVisible).not.toHaveBeenCalled()
  })

  it('measures the dwell from the first dropped status rather than the newest', async () => {
    const context = setup()
    context.notice.update(reachable)

    context.notice.update(chatEventsDropped)
    await vi.advanceTimersByTimeAsync(1_000)
    context.notice.update(disconnected)
    await vi.advanceTimersByTimeAsync(1_000)

    expect(context.onVisible).toHaveBeenCalledExactlyOnceWith(true)
  })

  it('reports no repeat while the notice stays up', async () => {
    const context = setup()
    context.notice.update(disconnected)

    context.notice.update(chatEventsDropped)
    await vi.advanceTimersByTimeAsync(5_000)

    expect(context.onVisible).toHaveBeenCalledExactlyOnceWith(true)
  })

  it('hides the notice on recovery and dwells again on the next drop', async () => {
    const context = setup()
    context.notice.update(disconnected)

    context.notice.update(reachable)
    expect(context.onVisible).toHaveBeenLastCalledWith(false)

    context.notice.update(disconnected)
    await vi.advanceTimersByTimeAsync(1_999)
    expect(context.onVisible).toHaveBeenLastCalledWith(false)
    await vi.advanceTimersByTimeAsync(1)
    expect(context.onVisible).toHaveBeenLastCalledWith(true)
    expect(context.onVisible).toHaveBeenCalledTimes(3)
  })

  it('drops a pending dwell during cleanup', async () => {
    const context = setup()
    context.notice.update(reachable)
    context.notice.update(disconnected)

    context.notice.cleanup()
    await vi.advanceTimersByTimeAsync(5_000)

    expect(context.onVisible).not.toHaveBeenCalled()
  })
})

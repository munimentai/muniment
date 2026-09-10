import { describe, expect, it, vi } from 'vitest'
import { createBackgroundServiceNotice, runtimeNotice } from './background-service-notice.js'

const snapshot = (revision, lastEvent, visible = true, busy = false) => ({ revision, lastEvent, visible, busy })

function setup(initial = snapshot(0, 'connected', false)) {
  let listener
  let poll
  const unlisten = vi.fn()
  const onChange = vi.fn()
  const invoke = vi.fn().mockResolvedValue(initial)
  const clearPoll = vi.fn()
  const notice = createBackgroundServiceNotice({
    invoke,
    listen: vi.fn(async (_, callback) => { listener = callback; return unlisten }),
    onChange,
    setPoll: (callback) => { poll = callback; return 1 },
    clearPoll,
  })
  return { notice, invoke, onChange, unlisten, clearPoll, event: (payload) => listener({ payload }), poll: () => poll() }
}

describe('runtime notice', () => {
  it.each([
    ['starting', 'The runtime received a start request.'],
    ['disconnected', 'The runtime connection closed.'],
    ['exited', 'The runtime exited.'],
    ['startFailed', 'The runtime start failed.'],
    ['requiresApproval', 'The runtime registration needs approval.'],
    ['approved', 'The runtime registration has approval.'],
    ['notFound', 'The runtime registration found no service.'],
    ['registrationFailed', 'The runtime registration failed.'],
    ['stopped', 'The runtime stopped.'],
    ['stopFailed', 'The runtime stop failed.'],
  ])('names the last event %s in one sentence', (event, text) => {
    const notice = runtimeNotice(snapshot(1, event))
    expect(notice.text).toBe(text)
    expect(text.split('.').filter(Boolean)).toHaveLength(1)
    expect(notice.control).toBe(event === 'requiresApproval' ? 'Open Login Items' : 'Start runtime')
  })

  it('covers exit, restart and recovery for two windows', async () => {
    const windows = [setup(), setup()]
    for (const window of windows) {
      await window.notice.start()
      window.event(snapshot(1, 'exited'))
      expect(window.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ text: 'The runtime exited.', visible: true }))
    }
    windows[0].invoke.mockResolvedValue(snapshot(2, 'starting', true, true))
    await windows[0].notice.retry()
    expect(windows[0].invoke).toHaveBeenCalledWith('runtime_start')
    expect(windows[1].invoke).not.toHaveBeenCalledWith('runtime_start')
    for (const window of windows) {
      window.event(snapshot(3, 'connected', false))
      expect(window.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ visible: false }))
      window.notice.cleanup()
    }
  })

  it('opens Login Items when registration needs approval', async () => {
    const window = setup(snapshot(1, 'requiresApproval'))
    await window.notice.start()
    await window.notice.retry()
    expect(window.invoke).toHaveBeenCalledWith('open_login_items')
    expect(window.invoke).not.toHaveBeenCalledWith('runtime_start')
  })

  it('keeps newer events when a stale read finishes', async () => {
    const window = setup()
    let resolve
    window.invoke.mockReturnValueOnce(new Promise((done) => { resolve = done }))
    const starting = window.notice.start()
    await Promise.resolve()
    window.event(snapshot(3, 'connected', false))
    resolve(snapshot(2, 'exited'))
    await starting
    expect(window.onChange).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ visible: false }))
    window.notice.cleanup()
  })

  it('ignores invalid state and preserves the owner visibility without a window dwell', async () => {
    const window = setup(snapshot(0, 'disconnected'))
    await window.notice.start()
    expect(window.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ visible: true }))
    for (const value of [null, {}, snapshot(-1, 'exited'), snapshot(NaN, 'exited'), snapshot(2, 'unknown'), snapshot(2, 'toString')]) window.event(value)
    expect(window.onChange).toHaveBeenCalledTimes(1)
    window.event(snapshot(2, 'disconnected', false))
    expect(window.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ visible: false }))
    window.notice.cleanup()
  })

  it('blocks duplicate actions and retries after a failed command', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    const window = setup(snapshot(1, 'startFailed'))
    await window.notice.start()
    let reject
    window.invoke.mockReturnValueOnce(new Promise((_, fail) => { reject = fail }))
    const retry = window.notice.retry()
    await window.notice.retry()
    expect(window.invoke.mock.calls.filter(([command]) => command === 'runtime_start')).toHaveLength(1)
    reject(new Error('IPC failed'))
    await retry
    await window.notice.retry()
    expect(window.invoke.mock.calls.filter(([command]) => command === 'runtime_start')).toHaveLength(2)
    window.event(snapshot(2, 'starting', true, true))
    await window.notice.retry()
    expect(window.invoke.mock.calls.filter(([command]) => command === 'runtime_start')).toHaveLength(2)
    window.notice.cleanup()
    vi.restoreAllMocks()
  })

  it('keeps a contradictory connected snapshot hidden', () => {
    expect(runtimeNotice(snapshot(1, 'connected', true)).visible).toBe(false)
  })

  it('registers once and releases a listener that arrives after cleanup', async () => {
    let resolve
    const unlisten = vi.fn()
    const invoke = vi.fn()
    const listen = vi.fn(() => new Promise((done) => { resolve = done }))
    const notice = createBackgroundServiceNotice({ invoke, listen, onChange: vi.fn() })
    const starting = notice.start()
    await notice.start()
    expect(listen).toHaveBeenCalledOnce()
    notice.cleanup()
    resolve(unlisten)
    await starting
    expect(unlisten).toHaveBeenCalledOnce()
    expect(invoke).not.toHaveBeenCalled()
    await notice.start()
    expect(listen).toHaveBeenCalledOnce()
  })

  it('cleans up listeners and ignores late status reads', async () => {
    const window = setup()
    await window.notice.start()
    window.notice.cleanup()
    window.event(snapshot(2, 'exited'))
    await window.poll()
    expect(window.onChange).toHaveBeenCalledTimes(1)
    expect(window.unlisten).toHaveBeenCalledOnce()
    expect(window.clearPoll).toHaveBeenCalledWith(1)
    expect(window.invoke).not.toHaveBeenCalledWith('runtime_stop')
  })
})

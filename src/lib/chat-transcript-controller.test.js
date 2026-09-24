// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createChatTranscriptController, transcriptKeys } from './chat-transcript-controller.js'
import { COPY_CONFIRMATION_MS } from './message-actions.js'

function deferred() {
  let resolve
  let reject
  const promise = new Promise((yes, no) => {
    resolve = yes
    reject = no
  })
  return { promise, resolve, reject }
}

function setup(overrides = {}) {
  let pinned = overrides.pinned ?? true
  let destroyed = false
  let expandedReceipts = new Set()
  let parallelTools = new Map()
  const thread = overrides.thread ?? {
    scrollTop: 0,
    scrollHeight: 1000,
    clientHeight: 400,
    scrollTo: vi.fn(),
  }
  const onPinned = vi.fn((next) => { pinned = next })
  const onContentBelow = vi.fn()
  const onCopy = vi.fn()
  const onExpandedReceipts = vi.fn((next) => { expandedReceipts = next })
  const onParallelTools = vi.fn((next) => { parallelTools = next })
  const controller = createChatTranscriptController({
    tick: overrides.tick ?? (() => Promise.resolve()),
    clipboard: overrides.clipboard ?? { writeText: vi.fn().mockResolvedValue(undefined) },
    readThread: () => thread,
    readPinned: () => pinned,
    readDestroyed: () => destroyed,
    onPinned,
    onContentBelow,
    onCopy,
    readExpandedReceipts: () => expandedReceipts,
    onExpandedReceipts,
    readParallelTools: () => parallelTools,
    onParallelTools,
    ...(overrides.frame ? { frame: overrides.frame } : {}),
  })
  return {
    controller,
    thread,
    onPinned,
    onContentBelow,
    onCopy,
    onExpandedReceipts,
    onParallelTools,
    expandedReceipts: () => expandedReceipts,
    parallelTools: () => parallelTools,
    setPinned: (next) => { pinned = next },
    setDestroyed: (next) => { destroyed = next },
  }
}

beforeEach(() => {
  window.matchMedia = vi.fn(() => ({ matches: false }))
})

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('chat transcript controller', () => {
  it('publishes a successful copy and clears it after the confirmation window', async () => {
    vi.useFakeTimers()
    const context = setup()

    await context.controller.copyResponse({ id: 'run-1', text: 'Answer' })

    expect(context.onCopy.mock.calls).toEqual([
      [null],
      [{ runId: 'run-1', status: 'copied' }],
    ])
    await vi.advanceTimersByTimeAsync(COPY_CONFIRMATION_MS)
    expect(context.onCopy).toHaveBeenLastCalledWith(null)
  })

  it('publishes a failed copy without scheduling a reset', async () => {
    vi.useFakeTimers()
    const clipboard = { writeText: vi.fn().mockRejectedValue(new Error('denied')) }
    const context = setup({ clipboard })

    await context.controller.copyResponse({ id: 'run-1', text: 'Answer' })

    expect(context.onCopy.mock.calls).toEqual([
      [null],
      [{ runId: 'run-1', status: 'failed' }],
    ])
    expect(vi.getTimerCount()).toBe(0)
  })

  it('lets a second copy invalidate the first pending timer', async () => {
    vi.useFakeTimers()
    const context = setup()
    await context.controller.copyResponse({ id: 'run-1', text: 'First' })

    await vi.advanceTimersByTimeAsync(COPY_CONFIRMATION_MS - 1)
    await context.controller.copyResponse({ id: 'run-2', text: 'Second' })
    await vi.advanceTimersByTimeAsync(1)

    expect(context.onCopy).toHaveBeenLastCalledWith({ runId: 'run-2', status: 'copied' })
    await vi.advanceTimersByTimeAsync(COPY_CONFIRMATION_MS - 1)
    expect(context.onCopy).toHaveBeenLastCalledWith(null)
  })

  it('drops a late continuation from an older copy epoch', async () => {
    const first = deferred()
    const clipboard = { writeText: vi.fn()
      .mockImplementationOnce(() => first.promise)
      .mockResolvedValueOnce(undefined) }
    const context = setup({ clipboard })

    const older = context.controller.copyResponse({ id: 'run-1', text: 'First' })
    await context.controller.copyResponse({ id: 'run-2', text: 'Second' })
    first.resolve()
    await older

    expect(context.onCopy).not.toHaveBeenCalledWith({ runId: 'run-1', status: 'copied' })
    expect(context.onCopy).toHaveBeenLastCalledWith({ runId: 'run-2', status: 'copied' })
    context.controller.cleanup()
  })

  it('publishes nothing after destruction', async () => {
    const copy = deferred()
    const context = setup({ clipboard: { writeText: vi.fn(() => copy.promise) } })

    const pending = context.controller.copyResponse({ id: 'run-1', text: 'Answer' })
    context.setDestroyed(true)
    copy.resolve()
    await pending

    expect(context.onCopy).not.toHaveBeenCalled()
  })

  it('groups concurrent running tools and accumulates their effect ids', () => {
    const context = setup()
    context.controller.trackParallelTools([assistant('run-1', [
      tool('tool-1'), tool('tool-2'), tool('done', 'completed'),
    ])])
    context.controller.trackParallelTools([assistant('run-1', [
      tool('tool-2'), tool('tool-3'),
    ])])

    expect(context.parallelTools().get('run-1')).toEqual(['tool-1', 'tool-2', 'tool-3'])
    expect(context.onParallelTools).toHaveBeenCalledTimes(2)
  })

  it('does not publish for a single running tool or unchanged group sizes', () => {
    const context = setup()
    context.controller.trackParallelTools([assistant('solo', [tool('tool-1')])])
    expect(context.parallelTools().has('solo')).toBe(false)
    expect(context.onParallelTools).not.toHaveBeenCalled()

    const messages = [assistant('run-1', [tool('tool-1'), tool('tool-2')])]
    context.controller.trackParallelTools(messages)
    context.onParallelTools.mockClear()
    context.controller.trackParallelTools(messages)

    expect(context.onParallelTools).not.toHaveBeenCalled()
  })

  it('adds and removes expanded receipts through fresh Sets', () => {
    const context = setup()
    context.controller.toggleReceipt('run-1')
    const added = context.expandedReceipts()
    expect([...added]).toEqual(['run-1'])

    context.controller.toggleReceipt('run-1')
    expect([...context.expandedReceipts()]).toEqual([])
    expect(context.onExpandedReceipts).toHaveBeenCalledTimes(2)
    expect(context.expandedReceipts()).not.toBe(added)
  })

  it('follows new content only while pinned', async () => {
    const context = setup()
    await context.controller.followNewContent()
    expect(context.thread.scrollTo).toHaveBeenCalledWith({ top: 1000, behavior: 'auto' })
    expect(context.onContentBelow).toHaveBeenLastCalledWith(false)

    context.thread.scrollTo.mockClear()
    context.setPinned(false)
    await context.controller.followNewContent()
    expect(context.thread.scrollTo).not.toHaveBeenCalled()
  })

  it('scrolls once per frame however many updates arrive in it', async () => {
    const frames = []
    const context = setup({ frame: (callback) => frames.push(callback) })
    const first = context.controller.followNewContent()
    const second = context.controller.followNewContent()
    expect(second).toBe(first)
    await Promise.resolve()
    await Promise.resolve()
    expect(context.controller.followNewContent()).toBe(first)
    expect(frames).toHaveLength(1)
    expect(context.thread.scrollTo).not.toHaveBeenCalled()

    frames[0]()
    await first
    expect(context.thread.scrollTo).toHaveBeenCalledOnce()

    const next = context.controller.followNewContent()
    expect(next).not.toBe(first)
    await Promise.resolve()
    await Promise.resolve()
    expect(frames).toHaveLength(2)
  })

  it('keys each transcript entry once and keeps a reply key when its run is named', () => {
    const user = { role: 'user', submissionId: 1, text: 'Hi' }
    const pending = { role: 'assistant', run: { id: 'pending', submissionId: 1 } }
    const named = { role: 'assistant', run: { id: 'run-1', submissionId: 1 } }
    const history = [{ role: 'user', id: 'run-0' }, { role: 'assistant', run: { id: 'run-0' } }]
    const queued = [{ role: 'user', text: 'one' }, { role: 'user', text: 'two' }]

    expect(transcriptKeys([user, pending]).get(pending)).toBe(transcriptKeys([user, named]).get(named))
    const repeated = { role: 'assistant', run: { id: 'run-0' } }
    const keys = transcriptKeys([...history, user, named, ...queued, repeated])
    expect(keys.size).toBe(7)
    expect(new Set(keys.values()).size).toBe(7)
    expect(keys.get(history[0])).not.toBe(keys.get(history[1]))
    expect(keys.get(queued[0])).not.toBe(keys.get(queued[1]))
  })

  it('scrolls to the latest content with the user motion preference', () => {
    const context = setup()
    context.controller.scrollToLatest()

    expect(window.matchMedia).toHaveBeenCalledWith('(prefers-reduced-motion: reduce)')
    expect(context.thread.scrollTo).toHaveBeenCalledWith({ top: 1000, behavior: 'smooth' })
    expect(context.onPinned).toHaveBeenCalledWith(true)
    expect(context.onContentBelow).toHaveBeenCalledWith(false)
  })

  it('rechecks pinned state after waiting for the DOM update', async () => {
    const update = deferred()
    const context = setup({ tick: () => update.promise })

    const following = context.controller.followNewContent()
    context.setPinned(false)
    update.resolve()
    await following

    expect(context.thread.scrollTo).not.toHaveBeenCalled()
  })

  it('publishes scroll-follow pinned state and whether content remains below', () => {
    const context = setup()
    context.controller.syncScrollTop(600)
    context.thread.scrollTop = 500
    context.controller.handleScroll()
    expect(context.onPinned).toHaveBeenLastCalledWith(false)
    expect(context.onContentBelow).toHaveBeenLastCalledWith(true)

    context.thread.scrollTop = 570
    context.controller.handleScroll()
    expect(context.onPinned).toHaveBeenLastCalledWith(true)
    expect(context.onContentBelow).toHaveBeenLastCalledWith(false)
  })
})

function tool(effectId, status = 'running') {
  return { effectId, status }
}

function assistant(id, toolActivity) {
  return { role: 'assistant', run: { id, toolActivity } }
}

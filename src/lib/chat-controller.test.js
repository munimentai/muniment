import { describe, expect, it, vi } from 'vitest'

import { createChatController } from './chat-controller.js'

function deferred() {
  let resolve
  let reject
  const promise = new Promise((yes, no) => {
    resolve = yes
    reject = no
  })
  return { promise, resolve, reject }
}

function setup(invoke = vi.fn(), { threadId = null, summaries = [] } = {}) {
  let messages = []
  let active = null
  let announced = null
  let draft = 'Hello'
  let files = []
  let openThreadId = threadId
  let threadSummaries = summaries
  let listener
  const errors = []
  const onHistoryError = vi.fn()
  const onMessages = vi.fn((next) => { messages = next })
  const onActive = vi.fn((next) => { active = next })
  const onThreadSummaries = vi.fn((next) => { threadSummaries = next })
  const onMoreThreads = vi.fn()
  const onThreadSelected = vi.fn()
  const onFreshThread = vi.fn()
  const onFocus = vi.fn()
  const onFollow = vi.fn()
  const controller = createChatController({
    invoke,
    listen: vi.fn(async (_, callback) => {
      listener = callback
      return vi.fn()
    }),
    readMessages: () => messages,
    readActive: () => active,
    readAnnounced: () => announced,
    readDraft: () => draft,
    readFiles: () => files,
    readThreadId: () => openThreadId,
    readThreadSummaries: () => threadSummaries,
    onMessages,
    onActive,
    onAnnounce: (next) => { announced = next },
    onDraft: (next) => { draft = next },
    onFiles: (next) => { files = next },
    onSubmitError: (error) => { if (error) errors.push(error) },
    onCancelError: vi.fn(),
    onQueueError: vi.fn(),
    onHistoryError,
    onThreadSummaries,
    onMoreThreads,
    onThreadSelected,
    onFreshThread,
    onFocus,
    onFollow,
  })
  return {
    controller,
    start: () => controller.start(),
    event: (payload) => listener({ payload }),
    messages: () => messages,
    active: () => active,
    announced: () => announced,
    draft: () => draft,
    errors,
    onMessages,
    onActive,
    onHistoryError,
    onThreadSummaries,
    onMoreThreads,
    onThreadSelected,
    onFreshThread,
    onFocus,
    onFollow,
    summaries: () => threadSummaries,
    setActive: (next) => { active = next },
    setDraft: (next) => { draft = next },
    setMessages: (next) => { messages = next },
    setThreadId: (next) => { openThreadId = next },
  }
}

describe('chat controller', () => {
  it('returns refreshThreads', () => {
    expect(setup().controller.refreshThreads).toEqual(expect.any(Function))
  })

  it('reports a rejected listener registration and retries it with history', async () => {
    const listen = vi.fn()
      .mockRejectedValueOnce(new Error('registration failed'))
      .mockResolvedValueOnce(vi.fn())
    const onHistoryError = vi.fn()
    const controller = createChatController({
      invoke: vi.fn().mockResolvedValue({ summaries: [] }),
      listen,
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      onMessages: vi.fn(),
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
    })

    await expect(controller.start()).resolves.toBe(false)
    expect(onHistoryError).toHaveBeenLastCalledWith('Live replies cannot arrive.', expect.objectContaining({ label: 'Reconnect' }))

    await controller.loadHistory()

    expect(listen).toHaveBeenCalledTimes(2)
    expect(onHistoryError).toHaveBeenLastCalledWith('')
  })

  it('deletes another thread without changing the transcript', async () => {
    const previous = [{ role: 'user', text: 'Keep this transcript' }]
    let summaries = [{ threadId: 'thread-1' }, { threadId: 'thread-2' }]
    const context = setup(vi.fn().mockResolvedValue(undefined))
    const invoke = vi.fn().mockResolvedValue(undefined)
    context.setMessages(previous)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      readThreadSummaries: () => summaries,
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSummaries: (next) => { summaries = next },
    })

    await expect(controller.deleteThread('thread-2')).resolves.toBe(true)

    expect(invoke).toHaveBeenCalledWith('chat_delete_thread', { threadId: 'thread-2' })
    expect(summaries).toEqual([{ threadId: 'thread-1' }])
    expect(context.messages()).toBe(previous)
  })

  it('clears the selected thread only after a successful delete', async () => {
    const request = deferred()
    const previous = [{ role: 'user', text: 'Delete this transcript' }]
    let summaries = [{ threadId: 'thread-1' }]
    const context = setup()
    context.setMessages(previous)
    const onHistoryError = vi.fn()
    const onFocus = vi.fn()
    const controller = createChatController({
      invoke: vi.fn(() => request.promise),
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      readThreadSummaries: () => summaries,
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
      onThreadSummaries: (next) => { summaries = next },
      onThreadSelected: context.onThreadSelected,
      onFreshThread: context.onFreshThread,
      onFocus,
    })

    const deleting = controller.deleteThread('thread-1')
    expect(context.messages()).toBe(previous)
    request.resolve()
    await expect(deleting).resolves.toBe(true)

    expect(context.messages()).toEqual([])
    expect(summaries).toEqual([])
    expect(context.onThreadSelected).toHaveBeenCalledWith(null)
    expect(context.onFreshThread).toHaveBeenCalledWith(true)
    expect(onFocus).toHaveBeenCalledOnce()

    context.setMessages(previous)
    const failedController = createChatController({
      invoke: vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue(undefined),
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      readThreadSummaries: () => [{ threadId: 'thread-1' }],
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
      onThreadSummaries: vi.fn(),
    })
    await expect(failedController.deleteThread('thread-1')).resolves.toBe(false)
    expect(context.messages()).toBe(previous)
    const deleteAction = onHistoryError.mock.lastCall[1]
    expect(deleteAction.label).toBe('Delete thread')
    await expect(deleteAction.run()).resolves.toBe(true)
  })

  it('renames the selected thread and publishes its new title', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined)
    const summaries = [
      { threadId: 'thread-1', title: 'Old name' },
      { threadId: 'thread-2', title: 'Other name' },
    ]
    const onThreadSummaries = vi.fn()
    const onHistoryError = vi.fn()
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      readThreadSummaries: () => summaries,
      onMessages: vi.fn(),
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
      onThreadSummaries,
    })

    await expect(controller.renameThread('  New name  ', 'Old name')).resolves.toBe(true)

    expect(invoke).toHaveBeenCalledWith('chat_rename_thread', {
      threadId: 'thread-1',
      title: 'New name',
    })
    expect(onThreadSummaries).toHaveBeenCalledWith([
      { threadId: 'thread-1', title: 'New name' },
      { threadId: 'thread-2', title: 'Other name' },
    ])
    expect(onHistoryError).toHaveBeenCalledWith('')
  })

  it('publishes a rename after the user switches threads', async () => {
    const request = deferred()
    let threadId = 'thread-1'
    let summaries = [
      { threadId: 'thread-1', title: 'Old name' },
      { threadId: 'thread-2', title: 'Other name' },
    ]
    const onThreadSummaries = vi.fn((next) => { summaries = next })
    const controller = createChatController({
      invoke: vi.fn(() => request.promise),
      listen: vi.fn(),
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => threadId,
      readThreadSummaries: () => summaries,
      onMessages: vi.fn(),
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSummaries,
    })

    const rename = controller.renameThread('New name', 'Old name')
    threadId = 'thread-2'
    request.resolve()

    await expect(rename).resolves.toBe(true)
    expect(onThreadSummaries).toHaveBeenCalledWith([
      { threadId: 'thread-1', title: 'New name' },
      { threadId: 'thread-2', title: 'Other name' },
    ])
  })

  it('serializes renames so a reload keeps the newer title', async () => {
    const first = deferred()
    const second = deferred()
    let summaries = [{ threadId: 'thread-1', title: 'Old name' }]
    let storedTitle = 'Old name'
    const onThreadSummaries = vi.fn((next) => { summaries = next })
    const invoke = vi.fn()
      .mockImplementationOnce(async () => {
        await first.promise
        storedTitle = 'Older name'
      })
      .mockImplementationOnce(async () => {
        await second.promise
        storedTitle = 'Newer name'
      })
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      readThreadSummaries: () => summaries,
      onMessages: vi.fn(),
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSummaries,
    })

    const olderRename = controller.renameThread('Older name', 'Old name')
    const newerRename = controller.renameThread('Newer name', 'Old name')
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1))
    second.resolve()
    expect(invoke).toHaveBeenCalledTimes(1)
    first.resolve()

    await expect(olderRename).resolves.toBe(true)
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2))
    await expect(newerRename).resolves.toBe(true)
    expect(onThreadSummaries).toHaveBeenCalledTimes(2)
    expect(summaries).toEqual([{ threadId: 'thread-1', title: 'Newer name' }])
    expect(storedTitle).toBe('Newer name')
  })

  it.each([
    [null, 'New name', 'Old name'],
    ['thread-1', '   ', 'Old name'],
    ['thread-1', ' Old name ', 'Old name'],
  ])('does not rename for thread %s and title %j', async (threadId, title, previousTitle) => {
    const invoke = vi.fn()
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => threadId,
      onMessages: vi.fn(),
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
    })

    await expect(controller.renameThread(title, previousTitle)).resolves.toBe(false)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('reports a rename failure without publishing a title', async () => {
    const onThreadSummaries = vi.fn()
    const onHistoryError = vi.fn()
    const controller = createChatController({
      invoke: vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue(undefined),
      listen: vi.fn(),
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      readThreadSummaries: () => [{ threadId: 'thread-1', title: 'Old name' }],
      onMessages: vi.fn(),
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
      onThreadSummaries,
    })

    await expect(controller.renameThread('New name', 'Old name')).resolves.toBe(false)

    expect(onThreadSummaries).not.toHaveBeenCalled()
    const renameAction = onHistoryError.mock.lastCall[1]
    expect(renameAction.label).toBe('Rename thread again')
    await expect(renameAction.run()).resolves.toBe(true)
    expect(onThreadSummaries).toHaveBeenLastCalledWith([{ threadId: 'thread-1', title: 'New name' }])
  })

  it('starts a fresh thread only after the command succeeds', async () => {
    const request = deferred()
    const previous = [{ role: 'user', text: 'Current transcript' }]
    const onThreadSelected = vi.fn()
    const onHistoryError = vi.fn()
    const onFocus = vi.fn()
    const invoke = vi.fn(() => request.promise)
    const context = setup()
    context.setMessages(previous)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
      onThreadSelected,
      onFocus,
    })

    const starting = controller.newThread()
    expect(context.messages()).toBe(previous)
    request.resolve()
    await starting

    expect(context.messages()).toEqual([])
    expect(invoke).toHaveBeenCalledOnce()
    expect(invoke).toHaveBeenCalledWith('chat_new_thread')
    expect(onThreadSelected).toHaveBeenCalledWith(null)
    expect(onFocus).toHaveBeenCalledOnce()
  })

  it('keeps the current thread when a fresh thread request fails', async () => {
    const previous = [{ role: 'user', text: 'Current transcript' }]
    const onThreadSelected = vi.fn()
    const onHistoryError = vi.fn()
    const context = setup(vi.fn().mockRejectedValue(new Error('offline')))
    context.setMessages(previous)
    const controller = createChatController({
      invoke: vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue(undefined),
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
      onThreadSelected,
    })

    await controller.newThread()

    expect(context.messages()).toBe(previous)
    expect(onThreadSelected).not.toHaveBeenCalled()
    const newThreadAction = onHistoryError.mock.lastCall[1]
    expect(newThreadAction.label).toBe('Start new thread')
    await newThreadAction.run()
    expect(context.messages()).toEqual([])
  })

  it('blocks a fresh thread during a run or thread switch', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke)
    context.setActive({ id: 'run-1' })

    await context.controller.newThread()
    expect(invoke).not.toHaveBeenCalled()

    context.setActive(null)
    const opening = context.controller.openThread('thread-2')
    await Promise.resolve()
    await context.controller.newThread()
    expect(invoke).not.toHaveBeenCalledWith('chat_new_thread')

    history.resolve({ entries: [], nextCursor: null })
    await opening
  })

  it('publishes an empty transcript without opening a missing thread', async () => {
    const invoke = vi.fn().mockResolvedValue({ summaries: [], nextCursor: null })
    const context = setup(invoke)

    await context.controller.loadHistory()

    expect(invoke).toHaveBeenCalledOnce()
    expect(invoke).toHaveBeenCalledWith('chat_thread_summaries', { limit: 20 })
    expect(context.messages()).toEqual([])
  })

  it('opens the newest thread on the first load after sign-in', async () => {
    const invoke = vi.fn(async (command) => command === 'chat_thread_summaries'
      ? { summaries: [{ threadId: 'thread-3' }, { threadId: 'thread-1' }], nextCursor: null }
      : { entries: [], nextCursor: null })
    const context = setup(invoke)

    await context.controller.loadHistory()

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_thread_open', { threadId: 'thread-3', limit: 100 }],
    ])
    expect(context.onThreadSelected).toHaveBeenLastCalledWith('thread-3')
  })

  it('reopens the thread the shell holds open when a reconnect reloads history', async () => {
    let currentThreadId = null
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-3' }, { threadId: 'thread-1' }], nextCursor: null }
      if (command === 'chat_thread_open') return { entries: [], nextCursor: null }
      return undefined
    })
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => currentThreadId,
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSelected: (next) => { currentThreadId = next },
    })

    await controller.loadHistory()
    await controller.openThread('thread-1')
    invoke.mockClear()
    await controller.loadHistory()

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
    ])
    expect(currentThreadId).toBe('thread-1')
  })

  it('keeps a fresh thread in place when a reconnect reloads history', async () => {
    let currentThreadId = 'thread-1'
    let freshThread = false
    const onMessages = vi.fn()
    const onThreadSummaries = vi.fn()
    const invoke = vi.fn(async (command) => command === 'chat_thread_summaries'
      ? { summaries: [{ threadId: 'thread-1' }], nextCursor: null }
      : undefined)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => currentThreadId,
      onMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSummaries,
      onThreadSelected: (next) => { currentThreadId = next },
      onFreshThread: (next) => { freshThread = next },
    })

    await controller.newThread()
    invoke.mockClear()
    onMessages.mockClear()

    await controller.loadHistory()

    expect(invoke.mock.calls).toEqual([['chat_thread_summaries', { limit: 20 }]])
    expect(onThreadSummaries).toHaveBeenLastCalledWith([{ threadId: 'thread-1' }])
    expect(onMessages).not.toHaveBeenCalled()
    expect(currentThreadId).toBeNull()
    expect(freshThread).toBe(true)
  })

  it('selects the newest thread when a blocked switch left a fresh thread open', async () => {
    let currentThreadId = null
    let openFails = true
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-1' }], nextCursor: null }
      if (command !== 'chat_thread_open') return undefined
      if (!openFails) return { entries: [], nextCursor: null }
      openFails = false
      throw new Error('offline')
    })
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => currentThreadId,
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSelected: (next) => { currentThreadId = next },
    })

    await controller.newThread()
    await controller.openThread('thread-1')
    invoke.mockClear()
    await controller.loadHistory()

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_select_thread', { threadId: 'thread-1' }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
    ])
    expect(currentThreadId).toBe('thread-1')
  })

  it('opens the newest thread when the thread the shell holds open no longer loads', async () => {
    let currentThreadId = null
    let summaries = [{ threadId: 'thread-3' }, { threadId: 'thread-1' }]
    const onHistoryError = vi.fn()
    const invoke = vi.fn(async (command, payload) => {
      if (command === 'chat_thread_summaries') return { summaries, nextCursor: null }
      if (command !== 'chat_thread_open') return undefined
      if (payload.threadId === 'thread-1' && summaries.length === 1) throw new Error('missing first run')
      return { entries: [], nextCursor: null }
    })
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => currentThreadId,
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
      onThreadSelected: (next) => { currentThreadId = next },
    })

    await controller.openThread('thread-1')
    // Retention prunes the open thread while the window sits idle.
    summaries = [{ threadId: 'thread-3' }]
    invoke.mockClear()
    onHistoryError.mockClear()
    await controller.loadHistory()

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
      ['chat_thread_open', { threadId: 'thread-3', limit: 100 }],
    ])
    expect(currentThreadId).toBe('thread-3')
    expect(onHistoryError).toHaveBeenLastCalledWith('')
  })

  it('reselects the thread the shell holds open when a blocked switch reloads history', async () => {
    let currentThreadId = null
    let openThreeFails = true
    let restoreFails = false
    const onThreadSwitch = vi.fn()
    const invoke = vi.fn(async (command, payload) => {
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-3' }, { threadId: 'thread-1' }], nextCursor: null }
      if (command === 'chat_select_thread') {
        if (!restoreFails) return undefined
        restoreFails = false
        throw new Error('offline')
      }
      if (command !== 'chat_thread_open') return undefined
      if (payload.threadId === 'thread-3' && openThreeFails) {
        openThreeFails = false
        restoreFails = true
        throw new Error('offline')
      }
      return { entries: [], nextCursor: null }
    })
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => currentThreadId,
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSelected: (next) => { currentThreadId = next },
      onThreadSwitch,
    })

    await controller.openThread('thread-1')
    // The switch to thread-3 fails and the restore fails too, so the backend
    // selection no longer matches the thread the shell holds open.
    await controller.openThread('thread-3')
    invoke.mockClear()
    await controller.loadHistory()

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_select_thread', { threadId: 'thread-1' }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
    ])
    expect(currentThreadId).toBe('thread-1')
    expect(onThreadSwitch).toHaveBeenLastCalledWith(false)
  })

  it('appends older threads and reports when the last page arrives', async () => {
    let summaries = [{ threadId: 'thread-1' }]
    const invoke = vi.fn()
      .mockResolvedValueOnce({ summaries, nextCursor: 'page-2' })
      .mockResolvedValueOnce({ entries: [], nextCursor: null })
      .mockResolvedValueOnce({ summaries: [{ threadId: 'thread-2' }], nextCursor: null })
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadSummaries: () => summaries,
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSummaries: (next) => { summaries = next },
      onMoreThreads: context.onMoreThreads,
    })

    await controller.loadHistory()
    await expect(controller.loadOlderThreads()).resolves.toBe('thread-2')

    expect(summaries).toEqual([{ threadId: 'thread-1' }, { threadId: 'thread-2' }])
    expect(invoke).toHaveBeenLastCalledWith('chat_thread_summaries', { limit: 20, cursor: 'page-2' })
    expect(context.onMoreThreads.mock.calls).toEqual([[true], [false]])
  })

  it('keeps shown threads when an older page fails', async () => {
    const summaries = [{ threadId: 'thread-1' }]
    const onThreadSummaries = vi.fn()
    const onHistoryError = vi.fn()
    const invoke = vi.fn()
      .mockResolvedValueOnce({ summaries, nextCursor: 'page-2' })
      .mockResolvedValueOnce({ entries: [], nextCursor: null })
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce({ summaries: [{ threadId: 'thread-2' }], nextCursor: null })
    const controller = createChatController({
      invoke, listen: vi.fn(), readMessages: () => [], readActive: () => null,
      readAnnounced: () => null, readDraft: () => '', readFiles: () => [],
      readThreadSummaries: () => summaries, onMessages: vi.fn(), onActive: vi.fn(),
      onAnnounce: vi.fn(), onDraft: vi.fn(), onFiles: vi.fn(), onSubmitError: vi.fn(),
      onCancelError: vi.fn(), onQueueError: vi.fn(), onHistoryError, onThreadSummaries,
    })

    await controller.loadHistory()
    onThreadSummaries.mockClear()
    await expect(controller.loadOlderThreads()).resolves.toBeNull()

    expect(onThreadSummaries).not.toHaveBeenCalled()
    const olderThreadsAction = onHistoryError.mock.lastCall[1]
    expect(olderThreadsAction.label).toBe('Load older threads')
    await expect(olderThreadsAction.run()).resolves.toBe('thread-2')
    expect(invoke).toHaveBeenLastCalledWith('chat_thread_summaries', { limit: 20, cursor: 'page-2' })
  })

  it('refreshes only the newest page and removes its threads from retained pages', async () => {
    let summaryCall = 0
    let summaries = []
    const invoke = vi.fn(async (command, payload) => {
      if (command === 'chat_thread_open') return { entries: [], nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-3'
      summaryCall += 1
      if (summaryCall === 1) return { summaries: [{ threadId: 'thread-1', title: 'First' }], nextCursor: 'page-2' }
      if (summaryCall === 2) return { summaries: [{ threadId: 'thread-2', title: 'Second' }], nextCursor: 'page-3' }
      if (summaryCall === 3) return { summaries: [{ threadId: 'thread-3', title: 'Third' }], nextCursor: 'page-4' }
      if (payload.cursor === undefined) return { summaries: [{ threadId: 'thread-3', title: 'Third updated' }], nextCursor: 'refresh-2' }
      return { summaries: [{ threadId: 'thread-4', title: 'Fourth' }], nextCursor: null }
    })
    const context = setup(invoke)
    let listener
    const controller = createChatController({
      invoke,
      listen: vi.fn(async (_, nextListener) => { listener = nextListener; return vi.fn() }),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      readThreadSummaries: () => summaries,
      onMessages: context.setMessages,
      onActive: (next) => context.setActive(next),
      onAnnounce: vi.fn(), onDraft: vi.fn(), onFiles: vi.fn(), onSubmitError: vi.fn(),
      onCancelError: vi.fn(), onQueueError: vi.fn(), onHistoryError: vi.fn(),
      onThreadSummaries: (next) => { summaries = next },
      onMoreThreads: context.onMoreThreads,
    })
    await controller.loadHistory()
    await controller.loadOlderThreads()
    await controller.loadOlderThreads()
    const run = { id: 'run-1', phase: 'streaming', text: '' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)
    await controller.start()
    invoke.mockClear()
    context.onMoreThreads.mockClear()

    listener({ payload: { runId: 'run-1', type: 'completed', receipt: null } })
    await vi.waitFor(() => expect(summaries).toEqual([
      { threadId: 'thread-3', title: 'Third updated' },
      { threadId: 'thread-1', title: 'First' },
      { threadId: 'thread-2', title: 'Second' },
    ]))
    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_current_thread'],
    ])
    expect(context.onMoreThreads).not.toHaveBeenCalled()

    await expect(controller.loadOlderThreads()).resolves.toBe('thread-4')
    expect(invoke).toHaveBeenLastCalledWith('chat_thread_summaries', { limit: 20, cursor: 'page-4' })
  })

  it('opens every history page in chronological page order', async () => {
    const first = {
      runId: 'run-1', phase: 'complete', text: 'First answer',
      prompt: 'First question', receipt: {}, toolActivity: [],
    }
    const second = {
      runId: 'run-2', phase: 'complete', text: 'Second answer',
      prompt: 'Second question', receipt: {}, toolActivity: [],
    }
    const invoke = vi.fn()
      .mockResolvedValueOnce({ summaries: [{ threadId: 'thread-1' }], nextCursor: null })
      .mockResolvedValueOnce({ entries: [first], nextCursor: 'page-2' })
      .mockResolvedValueOnce({ entries: [second], nextCursor: null })
    const context = setup(invoke)

    await context.controller.loadHistory()

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100, cursor: 'page-2' }],
    ])
    expect(context.messages().map((message) => message.text ?? message.run.text)).toEqual([
      'First question', 'First answer', 'Second question', 'Second answer',
    ])
  })

  it('rejoins the unsettled run of the opened thread without commanding it', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce({ summaries: [{ threadId: 'thread-1' }], nextCursor: null })
      .mockResolvedValueOnce({ entries: [
        { runId: 'run-1', phase: 'complete', text: 'First answer', prompt: 'First question', receipt: {}, toolActivity: [] },
        { runId: 'run-2', phase: 'streaming', text: 'Half an ans', prompt: 'Second question', receipt: null, toolActivity: [] },
      ], nextCursor: null })
    const context = setup(invoke)

    await context.controller.loadHistory()

    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an ans' })
    expect(context.announced()).toMatchObject({ id: 'run-2', phase: 'streaming' })
    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
    ])
  })

  it('leaves the active run null when every run in the opened thread settled', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({ entries: [
        { runId: 'run-1', phase: 'complete', text: 'Done', prompt: 'One', receipt: {}, toolActivity: [] },
        { runId: 'run-2', phase: 'interrupted', text: 'Partial', prompt: 'Two', receipt: null, toolActivity: [] },
      ], nextCursor: null })
    const context = setup(invoke)

    await context.controller.openThread('thread-2')

    expect(context.onActive).toHaveBeenCalledWith(null)
    expect(context.active()).toBeNull()
    expect(context.announced()).toBeNull()
    expect(invoke.mock.calls).toEqual([
      ['chat_select_thread', { threadId: 'thread-2' }],
      ['chat_thread_open', { threadId: 'thread-2', limit: 100 }],
    ])
  })

  it('settles a rejoined run from a chat event', async () => {
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_open') {
        return { entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an ans', prompt: 'Second question', receipt: null, toolActivity: [] }], nextCursor: null }
      }
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-2' }], nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-2'
      return undefined
    })
    const context = setup(invoke)
    await context.start()
    await context.controller.openThread('thread-2')

    context.event({ runId: 'run-2', type: 'text-delta', text: 'wer' })

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming' })

    context.event({ runId: 'run-2', type: 'completed', receipt: { route: 'local' } })

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'complete', text: 'Half an answer', receipt: { route: 'local' } })
    expect(context.announced()).toMatchObject({ id: 'run-2', phase: 'complete' })
    expect(context.active()).toBeNull()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_current_thread'))
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_resume', expect.anything())
  })

  it('carries a delta that arrives while the rejoined thread loads', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke)
    await context.start()

    const opening = context.controller.openThread('thread-2')
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    context.event({ runId: 'run-2', type: 'text-delta', text: 'wer' })
    history.resolve({ entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an ans', prompt: 'Second question', receipt: null, toolActivity: [] }], nextCursor: null })
    await opening

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.announced()).toMatchObject({ id: 'run-2', text: 'Half an answer' })
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_resume', expect.anything())
  })

  it('settles the rejoined run when its completion arrives while the thread loads', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_open') return history.promise
      if (command === 'chat_thread_summaries') return Promise.resolve({ summaries: [{ threadId: 'thread-2' }], nextCursor: null })
      if (command === 'chat_current_thread') return Promise.resolve('thread-2')
      return Promise.resolve()
    })
    const context = setup(invoke)
    await context.start()

    const opening = context.controller.openThread('thread-2')
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    context.event({ runId: 'run-2', type: 'completed', receipt: { route: 'local' } })
    history.resolve({ entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an answer', prompt: 'Second question', receipt: null, toolActivity: [] }], nextCursor: null })
    await opening

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'complete', text: 'Half an answer', receipt: { route: 'local' } })
    expect(context.announced()).toMatchObject({ id: 'run-2', phase: 'complete' })
    expect(context.onActive).toHaveBeenLastCalledWith(null)
    expect(context.active()).toBeNull()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_current_thread'))
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_resume', expect.anything())
  })

  it('carries a delta that lands while the thread loads over a live transcript', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke)
    await context.start()
    context.setMessages([{ role: 'assistant', run: { id: 'run-2', phase: 'streaming', text: 'Half an ans', receipt: null, toolActivity: [] } }])

    const opening = context.controller.openThread('thread-2')
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    context.event({ runId: 'run-2', type: 'text-delta', text: 'wer' })
    history.resolve({ entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an ans', prompt: 'Second question', receipt: null, toolActivity: [] }], nextCursor: null })
    await opening

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_resume', expect.anything())
  })

  it('settles the rejoined run when its completion lands over a live transcript', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_open') return history.promise
      if (command === 'chat_thread_summaries') return Promise.resolve({ summaries: [{ threadId: 'thread-2' }], nextCursor: null })
      if (command === 'chat_current_thread') return Promise.resolve('thread-2')
      return Promise.resolve()
    })
    const context = setup(invoke)
    await context.start()
    context.setMessages([{ role: 'assistant', run: { id: 'run-2', phase: 'streaming', text: 'Half an answer', receipt: null, toolActivity: [] } }])

    const opening = context.controller.openThread('thread-2')
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    context.event({ runId: 'run-2', type: 'completed', receipt: { route: 'local' } })
    history.resolve({ entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an answer', prompt: 'Second question', receipt: null, toolActivity: [] }], nextCursor: null })
    await opening

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'complete', text: 'Half an answer', receipt: { route: 'local' } })
    expect(context.onActive).toHaveBeenLastCalledWith(null)
    expect(context.active()).toBeNull()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_current_thread'))
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_resume', expect.anything())
  })

  it('selects another thread and publishes it only after every page loads', async () => {
    const previous = [{ role: 'user', text: 'Current transcript' }]
    const onThreadSelected = vi.fn()
    const invoke = vi.fn()
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({ entries: [{
        runId: 'run-2', phase: 'complete', text: 'New answer',
        prompt: 'New question', receipt: {}, toolActivity: [],
      }], nextCursor: null })
    const context = setup(invoke)
    context.setMessages(previous)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
      onThreadSelected,
    })

    await controller.openThread('thread-2')

    expect(invoke.mock.calls).toEqual([
      ['chat_select_thread', { threadId: 'thread-2' }],
      ['chat_thread_open', { threadId: 'thread-2', limit: 100 }],
    ])
    expect(context.messages().map((message) => message.text ?? message.run.text)).toEqual([
      'New question', 'New answer',
    ])
    expect(onThreadSelected).toHaveBeenCalledWith('thread-2')
  })

  it('blocks a send while another thread loads', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_open') return history.promise
      return Promise.resolve()
    })
    const context = setup(invoke)

    const opening = context.controller.openThread('thread-2')
    await Promise.resolve()
    await context.controller.send()

    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    history.resolve({ entries: [], nextCursor: null })
    await opening
  })

  it.each(['chat_select_thread', 'chat_thread_open'])('keeps the transcript when %s fails', async (failedCommand) => {
    const previous = [{ role: 'user', text: 'Current transcript' }]
    const onHistoryError = vi.fn()
    const context = setup()
    context.setMessages(previous)
    const controller = createChatController({
      invoke: vi.fn(async (command) => {
        if (command === failedCommand) throw new Error('offline')
        return command === 'chat_thread_open' ? { entries: [], nextCursor: null } : undefined
      }),
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
    })

    await controller.openThread('thread-2')

    expect(context.messages()).toBe(previous)
    expect(onHistoryError).toHaveBeenLastCalledWith('Conversation history could not be restored.', expect.objectContaining({ label: 'Restore history' }))
  })

  it('restores the previous backend thread when the selected thread fails to load', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce()
      .mockResolvedValueOnce({ runId: 'run-1' })
    const context = setup()
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => 'Send after recovery',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
    })

    await controller.openThread('thread-2')
    await controller.send()

    expect(invoke.mock.calls.slice(0, 3)).toEqual([
      ['chat_select_thread', { threadId: 'thread-2' }],
      ['chat_thread_open', { threadId: 'thread-2', limit: 100 }],
      ['chat_select_thread', { threadId: 'thread-1' }],
    ])
    expect(invoke).toHaveBeenCalledWith('chat_submit', {
      prompt: 'Send after recovery',
      files: [],
    })
  })

  it('blocks sends after restoration fails until a thread retry succeeds', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce()
      .mockRejectedValueOnce(new Error('page offline'))
      .mockRejectedValueOnce(new Error('restore offline'))
      .mockResolvedValueOnce()
      .mockResolvedValueOnce({ entries: [], nextCursor: null })
      .mockResolvedValueOnce({ runId: 'run-1' })
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => 'Do not send',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
    })

    await controller.openThread('thread-2')
    await controller.send()

    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())

    await controller.openThread('thread-1')
    await controller.send()

    expect(invoke).toHaveBeenCalledWith('chat_submit', {
      prompt: 'Do not send',
      files: [],
    })
  })

  it('keeps sends blocked when a thread selection retry fails', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce()
      .mockRejectedValueOnce(new Error('page offline'))
      .mockRejectedValueOnce(new Error('restore offline'))
      .mockRejectedValueOnce(new Error('select offline'))
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => 'Do not send',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
    })

    await controller.openThread('thread-2')
    await controller.openThread('thread-1')
    await controller.send()

    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
  })

  it('selects and loads a known thread before a blocked history retry allows sends', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce()
      .mockRejectedValueOnce(new Error('page offline'))
      .mockRejectedValueOnce(new Error('restore offline'))
      .mockResolvedValueOnce({ summaries: [{ threadId: 'thread-1' }], nextCursor: null })
      .mockResolvedValueOnce()
      .mockResolvedValueOnce({ entries: [], nextCursor: null })
      .mockResolvedValueOnce({ runId: 'run-1' })
    const context = setup(invoke)
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: context.messages,
      readActive: context.active,
      readAnnounced: () => null,
      readDraft: () => 'Send after retry',
      readFiles: () => [],
      readThreadId: () => 'thread-1',
      onMessages: context.setMessages,
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError: vi.fn(),
    })

    await controller.openThread('thread-2')
    await controller.loadHistory()
    await controller.send()

    expect(invoke.mock.calls.slice(3, 6)).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_select_thread', { threadId: 'thread-1' }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
    ])
    expect(invoke).toHaveBeenCalledWith('chat_submit', {
      prompt: 'Send after retry',
      files: [],
    })
  })

  it('does not select a thread while a run is active', async () => {
    const invoke = vi.fn()
    const context = setup(invoke)
    context.setActive({ id: 'run-1', phase: 'streaming' })

    await context.controller.openThread('thread-2')

    expect(invoke).not.toHaveBeenCalled()
  })

  it('re-reads every page of the open thread while a run stays active', async () => {
    const invoke = vi.fn(async (command, payload) => {
      if (command !== 'chat_thread_open') throw new Error(`unexpected command: ${command}`)
      return payload.cursor === undefined
        ? { entries: [{ runId: 'run-1', phase: 'complete', text: 'First answer', prompt: 'First question', receipt: {}, toolActivity: [] }], nextCursor: 'page-2' }
        : { entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an answer', prompt: 'Second question', receipt: null, toolActivity: [] }], nextCursor: null }
    })
    const context = setup(invoke, { threadId: 'thread-1' })
    const run = { id: 'run-2', phase: 'streaming', text: 'Half an' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    await context.controller.refreshOpenThread()

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
      ['chat_thread_open', { threadId: 'thread-1', limit: 100, cursor: 'page-2' }],
    ])
    expect(context.messages().map((message) => message.text ?? message.run.text)).toEqual([
      'First question', 'First answer', 'Second question', 'Half an answer',
    ])
    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.onThreadSelected).not.toHaveBeenCalled()
    expect(context.draft()).toBe('Hello')
  })

  it('carries an event that arrives while the open thread re-reads', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke, { threadId: 'thread-1' })
    await context.start()
    const run = { id: 'run-2', phase: 'streaming', text: 'Half an ans' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    context.event({ runId: 'run-2', type: 'text-delta', text: 'wer' })
    history.resolve({ entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an ans', prompt: 'Second question', receipt: null, toolActivity: [] }], nextCursor: null })
    await refreshing

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
  })

  it('ends the active run when the re-read finds its lost settlement', async () => {
    const invoke = vi.fn(async (command) => {
      if (command !== 'chat_thread_open') throw new Error(`unexpected command: ${command}`)
      return { entries: [{ runId: 'run-2', phase: 'complete', text: 'Whole answer', prompt: 'Second question', receipt: { route: 'local' }, toolActivity: [] }], nextCursor: null }
    })
    const context = setup(invoke, { threadId: 'thread-1' })
    const run = { id: 'run-2', phase: 'streaming', text: 'Half an' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    await context.controller.refreshOpenThread()

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'complete', text: 'Whole answer' })
    expect(context.onActive).toHaveBeenLastCalledWith(null)
    expect(context.active()).toBeNull()
  })

  it('leaves the transcript and the active run unchanged when the re-read fails', async () => {
    const invoke = vi.fn(async () => { throw new Error('offline') })
    const context = setup(invoke, { threadId: 'thread-1' })
    const run = { id: 'run-2', phase: 'streaming', text: 'Half an' }
    const previous = [{ role: 'assistant', run }]
    context.setMessages(previous)
    context.setActive(run)

    await context.controller.refreshOpenThread()

    expect(context.messages()).toBe(previous)
    expect(context.active()).toBe(run)
    expect(context.onMessages).not.toHaveBeenCalled()
    expect(context.onActive).not.toHaveBeenCalled()
    expect(context.onHistoryError).not.toHaveBeenCalled()
  })

  it('discards a re-read whose thread closed while its pages loaded', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke, { threadId: 'thread-1' })
    const previous = [{ role: 'user', text: 'Current transcript' }]
    context.setMessages(previous)

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    context.setThreadId('thread-2')
    history.resolve({ entries: [{ runId: 'run-1', phase: 'complete', text: 'Stale answer', prompt: 'Stale question', receipt: {}, toolActivity: [] }], nextCursor: null })
    await refreshing

    expect(context.messages()).toBe(previous)
    expect(context.onMessages).not.toHaveBeenCalled()
  })

  it('abandons a re-read that a send replaced, and keeps the sent run and its events', async () => {
    const history = deferred()
    const submit = deferred()
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_open') return history.promise
      if (command === 'chat_submit') return submit.promise
      return Promise.resolve()
    })
    const context = setup(invoke, { threadId: 'thread-1' })
    await context.start()
    context.setMessages([
      { role: 'user', text: 'First question' },
      { role: 'assistant', run: { id: 'run-1', phase: 'complete', text: 'First answer' } },
    ])

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    const sending = context.controller.send()
    history.resolve({ entries: [{ runId: 'run-1', phase: 'complete', text: 'First answer', prompt: 'First question', receipt: {}, toolActivity: [] }], nextCursor: null })
    await refreshing
    context.event({ runId: 'run-9', type: 'text-delta', text: 'Reply' })
    submit.resolve({ runId: 'run-9' })
    await sending
    context.event({ runId: 'run-9', type: 'text-delta', text: ' more' })

    expect(context.messages().map((message) => message.text ?? message.run.text)).toEqual([
      'First question', 'First answer', 'Hello', 'Reply more',
    ])
    expect(context.active()).toMatchObject({ id: 'run-9', phase: 'streaming', text: 'Reply more' })
  })

  it('abandons a re-read that a resume replaced, and keeps the resuming run', async () => {
    const history = deferred()
    const resume = deferred()
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_open') return history.promise
      if (command === 'chat_resume') return resume.promise
      return Promise.resolve()
    })
    const context = setup(invoke, { threadId: 'thread-1' })
    await context.start()
    const interrupted = { id: 'run-1', phase: 'interrupted', text: 'Half an', resumable: true }
    context.setMessages([{ role: 'assistant', run: interrupted }])

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    const resuming = context.controller.resume(interrupted)
    history.resolve({ entries: [{ runId: 'run-1', phase: 'interrupted', text: 'Half an', prompt: 'A question', receipt: null, toolActivity: [], resumable: true }], nextCursor: null })
    await refreshing

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-1', phase: 'resuming' })
    expect(context.active()).toMatchObject({ id: 'run-1', phase: 'resuming' })

    resume.resolve()
    await resuming

    expect(context.active()).toMatchObject({ id: 'run-1', phase: 'resuming' })
    expect(context.onActive).not.toHaveBeenCalledWith(null)
  })

  it('carries an event onto the visible run when a queued message abandons the re-read', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke, { threadId: 'thread-1' })
    await context.start()
    const run = { id: 'run-2', phase: 'streaming', text: 'Half an ans' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    await context.controller.queue('now')
    context.event({ runId: 'run-2', type: 'text-delta', text: 'wer' })
    history.resolve({ entries: [{ runId: 'run-2', phase: 'streaming', text: 'Half an ans', prompt: 'A question', receipt: null, toolActivity: [] }], nextCursor: null })
    await refreshing

    expect(context.messages().map((message) => message.text ?? message.run.text)).toEqual(['Half an answer', 'Hello'])
    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
  })

  it('carries an event that arrived while a failed re-read loaded its pages', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke, { threadId: 'thread-1' })
    await context.start()
    const run = { id: 'run-2', phase: 'streaming', text: 'Half an ans' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', expect.anything()))
    context.event({ runId: 'run-2', type: 'text-delta', text: 'wer' })
    history.reject(new Error('offline'))
    await refreshing

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.active()).toMatchObject({ id: 'run-2', phase: 'streaming', text: 'Half an answer' })
    expect(context.onHistoryError).not.toHaveBeenCalled()

    context.event({ runId: 'run-2', type: 'text-delta', text: '!' })

    expect(context.messages().at(-1).run.text).toBe('Half an answer!')
  })

  it('re-reads nothing when no thread is open', async () => {
    const invoke = vi.fn()
    const context = setup(invoke)

    await context.controller.refreshOpenThread()

    expect(invoke).not.toHaveBeenCalled()
  })

  it('re-reads nothing while a thread switch runs', async () => {
    const history = deferred()
    const invoke = vi.fn((command) => command === 'chat_thread_open' ? history.promise : Promise.resolve())
    const context = setup(invoke, { threadId: 'thread-1' })

    const opening = context.controller.openThread('thread-2')
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', { threadId: 'thread-2', limit: 100 }))
    await context.controller.refreshOpenThread()

    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', { threadId: 'thread-1', limit: 100 })
    history.resolve({ entries: [], nextCursor: null })
    await opening
  })

  it('re-reads nothing while another re-read runs', async () => {
    const history = deferred()
    const invoke = vi.fn(() => history.promise)
    const context = setup(invoke, { threadId: 'thread-1' })

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1))
    await context.controller.refreshOpenThread()

    expect(invoke).toHaveBeenCalledTimes(1)
    history.resolve({ entries: [], nextCursor: null })
    await refreshing
  })

  it('re-reads nothing while a submission waits for its run id', async () => {
    const submit = deferred()
    const invoke = vi.fn(() => submit.promise)
    const context = setup(invoke, { threadId: 'thread-1' })

    const sending = context.controller.send()
    await context.controller.refreshOpenThread()

    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', expect.anything())
    submit.resolve({ runId: 'run-1' })
    await sending
  })

  it('stops opening history after 100 pages and publishes them in order', async () => {
    const entries = Array.from({ length: 100 }, (_, page) => ({
      runId: `run-${page + 1}`,
      phase: 'complete',
      text: `Answer ${page + 1}`,
      prompt: `Question ${page + 1}`,
      receipt: {},
      toolActivity: [],
    }))
    const invoke = vi.fn(async (command, payload) => {
      if (command === 'chat_thread_summaries') {
        return { summaries: [{ threadId: 'thread-1' }], nextCursor: null }
      }
      const page = payload.cursor === undefined ? 0 : Number(payload.cursor)
      return { entries: [entries[page]], nextCursor: String(page + 1) }
    })
    const context = setup(invoke)

    await context.controller.loadHistory()

    const openCalls = invoke.mock.calls.filter(([command]) => command === 'chat_thread_open')
    expect(openCalls).toHaveLength(100)
    expect(openCalls.at(-1)).toEqual([
      'chat_thread_open',
      { threadId: 'thread-1', limit: 100, cursor: '99' },
    ])
    expect(context.messages().map((message) => message.text ?? message.run.text)).toEqual(
      entries.flatMap((entry) => [entry.prompt, entry.text]),
    )
  })

  it.each(['chat_thread_summaries', 'chat_thread_open'])('reports a %s failure with the current copy', async (failedCommand) => {
    const onHistoryError = vi.fn()
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') {
        if (failedCommand === command) throw new Error('offline')
        return { summaries: [{ threadId: 'thread-1' }], nextCursor: null }
      }
      throw new Error('offline')
    })
    const controller = createChatController({
      invoke,
      listen: vi.fn(),
      readMessages: () => [],
      readActive: () => null,
      readAnnounced: () => null,
      readDraft: () => '',
      readFiles: () => [],
      onMessages: vi.fn(),
      onActive: vi.fn(),
      onAnnounce: vi.fn(),
      onDraft: vi.fn(),
      onFiles: vi.fn(),
      onSubmitError: vi.fn(),
      onCancelError: vi.fn(),
      onQueueError: vi.fn(),
      onHistoryError,
    })
    await controller.loadHistory()

    expect(onHistoryError).toHaveBeenLastCalledWith('Conversation history could not be restored.', expect.objectContaining({ label: 'Restore history' }))
  })

  it('replays events buffered before a submitted run id is known', async () => {
    const submit = deferred()
    const context = setup(vi.fn(() => submit.promise))
    await context.start()
    const sending = context.controller.send()
    context.event({ runId: 'run-1', type: 'text-delta', text: 'Early' })
    context.event({ runId: 'run-1', type: 'completed', receipt: { route: 'local' } })
    submit.resolve({ runId: 'run-1' })
    await sending

    expect(context.messages().at(-1).run).toMatchObject({
      id: 'run-1',
      phase: 'complete',
      text: 'Early',
      receipt: { route: 'local' },
    })
    expect(context.active()).toBeNull()
  })

  it('drops events for a run this window never started', async () => {
    const submit = deferred()
    const context = setup(vi.fn(() => submit.promise))
    await context.start()

    context.event({ runId: 'run-9', type: 'prompt-accepted' })
    context.event({ runId: 'run-9', type: 'text-delta', text: 'Another' })
    context.event({ runId: 'run-9', type: 'text-delta', text: ' surface' })
    context.event({ runId: 'run-9', type: 'completed', receipt: { route: 'local' } })
    const sending = context.controller.send()
    submit.resolve({ runId: 'run-9' })
    await sending

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-9', phase: 'thinking', text: '', receipt: null })
    expect(context.active()).toMatchObject({ id: 'run-9', phase: 'thinking' })
  })

  it('refreshes the thread list once when an event names a new thread', async () => {
    const previous = [{ role: 'user', text: 'Keep this transcript' }]
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') {
        return { summaries: [{ threadId: 'thread-2', title: 'ACP' }, { threadId: 'thread-1' }], nextCursor: null }
      }
      if (command === 'chat_current_thread') return 'thread-1'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries: [{ threadId: 'thread-1' }] })
    context.setMessages(previous)
    await context.start()

    context.event({ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: 'thread-2' })
    await vi.waitFor(() => expect(context.summaries()).toEqual([
      { threadId: 'thread-2', title: 'ACP' },
      { threadId: 'thread-1' },
    ]))

    expect(context.messages()).toBe(previous)
    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_current_thread'],
    ])
    expect(context.onThreadSelected).toHaveBeenCalledWith('thread-1')

    context.event({ runId: 'run-9', type: 'text-delta', text: ' more', threadId: 'thread-2' })
    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_current_thread'],
    ])
  })

  it('refreshes once when a new thread settles the open run', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-2' }], nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-2'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { threadId: 'thread-2' })
    await context.start()
    const run = { id: 'run-1', phase: 'streaming', text: 'Done' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    context.event({ runId: 'run-1', type: 'completed', threadId: 'thread-2' })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_current_thread'))

    expect(context.active()).toBeNull()
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_thread_summaries')).toHaveLength(1)
    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_select_thread', expect.anything())
  })

  it('refreshes on settlement when the summaries already hold the thread', async () => {
    const summaries = [{ threadId: 'thread-1', title: 'Hello' }]
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_summaries') return { summaries, nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-1'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries })
    await context.start()
    const run = { id: 'run-1', phase: 'streaming', text: 'Done' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    context.event({ runId: 'run-1', type: 'completed', threadId: 'thread-1' })
    await vi.waitFor(() => expect(context.onThreadSelected).toHaveBeenCalledWith('thread-1'))

    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_current_thread'],
    ])
  })

  it('does not refresh again for a later event with the same thread id', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-1' }], nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-1'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { summaries: [{ threadId: 'thread-1' }] })
    await context.start()

    context.event({ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: 'thread-2' })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_summaries', { limit: 20 }))
    context.event({ runId: 'run-9', type: 'text-delta', text: ' more', threadId: 'thread-2' })

    expect(invoke.mock.calls.filter(([command]) => command === 'chat_thread_summaries')).toHaveLength(1)
  })

  it.each([
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP' }],
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: null }],
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: '' }],
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: 'thread-1' }],
  ])('does not refresh for event %j', async (payload) => {
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-1' }], nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-1'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { summaries: [{ threadId: 'thread-1' }] })
    await context.start()

    context.event(payload)

    expect(invoke).not.toHaveBeenCalled()
  })

  it('collapses signaled-thread refreshes that arrive while one is in flight', async () => {
    const first = deferred()
    const second = deferred()
    let summaryCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') {
        summaryCalls += 1
        if (summaryCalls === 1) await first.promise
        else await second.promise
        return { summaries: [{ threadId: 'thread-1' }], nextCursor: null }
      }
      if (command === 'chat_current_thread') return 'thread-1'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { summaries: [{ threadId: 'thread-1' }] })
    await context.start()

    context.event({ runId: 'run-9', type: 'text-delta', text: 'One', threadId: 'thread-2' })
    await vi.waitFor(() => expect(summaryCalls).toBe(1))
    context.event({ runId: 'run-10', type: 'text-delta', text: 'Two', threadId: 'thread-3' })
    context.event({ runId: 'run-11', type: 'text-delta', text: 'Three', threadId: 'thread-4' })
    expect(summaryCalls).toBe(1)
    first.resolve()
    await vi.waitFor(() => expect(summaryCalls).toBe(2))
    second.resolve()
    await vi.waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'chat_thread_summaries')).toHaveLength(2))
    expect(summaryCalls).toBe(2)
  })

  it('drops the oldest signaled thread past 256 entries', async () => {
    const first = deferred()
    let summaryCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') {
        summaryCalls += 1
        if (summaryCalls === 1) await first.promise
        return { summaries: [{ threadId: 'held' }], nextCursor: null }
      }
      if (command === 'chat_current_thread') return 'held'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { summaries: [{ threadId: 'held' }] })
    await context.start()

    for (let index = 0; index < 256; index += 1) {
      context.event({ runId: `run-${index}`, type: 'text-delta', text: 'x', threadId: `thread-${index}` })
      if (index === 0) await vi.waitFor(() => expect(summaryCalls).toBe(1))
    }
    expect(summaryCalls).toBe(1)
    first.resolve()
    await vi.waitFor(() => expect(context.onThreadSummaries).toHaveBeenCalledTimes(2))
    expect(summaryCalls).toBe(2)

    context.event({ runId: 'run-0', type: 'text-delta', text: 'x', threadId: 'thread-0' })
    expect(summaryCalls).toBe(2)

    context.event({ runId: 'run-256', type: 'text-delta', text: 'x', threadId: 'thread-256' })
    await vi.waitFor(() => expect(summaryCalls).toBe(3))

    context.event({ runId: 'run-0', type: 'text-delta', text: 'x', threadId: 'thread-0' })
    await vi.waitFor(() => expect(summaryCalls).toBe(4))

    context.event({ runId: 'run-256', type: 'text-delta', text: 'x', threadId: 'thread-256' })
    expect(summaryCalls).toBe(4)
  })

  it('re-reads the open thread once when an event names a new run on that thread', async () => {
    const invoke = vi.fn(async (command) => {
      if (command !== 'chat_thread_open') throw new Error(`unexpected command: ${command}`)
      return {
        entries: [{
          runId: 'run-9', phase: 'streaming', text: 'ACP',
          prompt: 'From the phone', receipt: null, toolActivity: [],
        }],
        nextCursor: null,
      }
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries: [{ threadId: 'thread-1' }] })
    context.setMessages([{ role: 'user', text: 'Keep this transcript' }])
    await context.start()

    context.event({ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: 'thread-1' })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', { threadId: 'thread-1', limit: 100 }))

    expect(context.messages().map((message) => message.text ?? message.run.text)).toEqual([
      'From the phone', 'ACP',
    ])
    expect(context.active()).toMatchObject({ id: 'run-9', phase: 'streaming', text: 'ACP' })
    expect(context.onThreadSelected).not.toHaveBeenCalled()
    expect(invoke).not.toHaveBeenCalledWith('chat_select_thread', expect.anything())

    context.event({ runId: 'run-9', type: 'text-delta', text: ' more', threadId: 'thread-1' })
    expect(invoke.mock.calls).toEqual([
      ['chat_thread_open', { threadId: 'thread-1', limit: 100 }],
    ])
    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-9', phase: 'streaming', text: 'ACP more' })
  })

  it('does not re-read again for a run id the set already holds', async () => {
    const invoke = vi.fn(async (command) => {
      if (command !== 'chat_thread_open') throw new Error(`unexpected command: ${command}`)
      return { entries: [], nextCursor: null }
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries: [{ threadId: 'thread-1' }] })
    await context.start()

    context.event({ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: 'thread-1' })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1))
    context.event({ runId: 'run-9', type: 'text-delta', text: ' more', threadId: 'thread-1' })

    expect(invoke).toHaveBeenCalledTimes(1)
    expect(context.messages()).toEqual([])
  })

  it.each([
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP' }],
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: null }],
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: '' }],
    [{ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: 'thread-2' }],
    [{ runId: 'run-1', type: 'text-delta', text: 'ACP', threadId: 'thread-1' }],
  ])('does not re-read the open thread for event %j', async (payload) => {
    const invoke = vi.fn(async (command) => {
      if (command === 'chat_thread_summaries') return { summaries: [{ threadId: 'thread-2' }], nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-1'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries: [{ threadId: 'thread-1' }] })
    await context.start()
    context.setMessages([{ role: 'assistant', run: { id: 'run-1', phase: 'complete', text: 'Done' } }])

    context.event(payload)
    await Promise.resolve()

    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', expect.anything())
  })

  it('collapses signaled-run re-reads that arrive while one is in flight', async () => {
    const first = deferred()
    const second = deferred()
    let openCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command !== 'chat_thread_open') throw new Error(`unexpected command: ${command}`)
      openCalls += 1
      if (openCalls === 1) await first.promise
      else await second.promise
      return { entries: [], nextCursor: null }
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries: [{ threadId: 'thread-1' }] })
    await context.start()

    context.event({ runId: 'run-9', type: 'text-delta', text: 'One', threadId: 'thread-1' })
    await vi.waitFor(() => expect(openCalls).toBe(1))
    context.event({ runId: 'run-10', type: 'text-delta', text: 'Two', threadId: 'thread-1' })
    context.event({ runId: 'run-11', type: 'text-delta', text: 'Three', threadId: 'thread-1' })
    expect(openCalls).toBe(1)
    first.resolve()
    await vi.waitFor(() => expect(openCalls).toBe(2))
    second.resolve()
    await vi.waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'chat_thread_open')).toHaveLength(2))
    expect(openCalls).toBe(2)
  })

  it('drops the oldest signaled run past 256 entries', async () => {
    const first = deferred()
    let openCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command !== 'chat_thread_open') throw new Error(`unexpected command: ${command}`)
      openCalls += 1
      if (openCalls === 1) await first.promise
      return { entries: [], nextCursor: null }
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries: [{ threadId: 'thread-1' }] })
    await context.start()

    for (let index = 0; index < 256; index += 1) {
      context.event({ runId: `run-${index}`, type: 'text-delta', text: 'x', threadId: 'thread-1' })
      if (index === 0) await vi.waitFor(() => expect(openCalls).toBe(1))
    }
    expect(openCalls).toBe(1)
    first.resolve()
    await vi.waitFor(() => expect(openCalls).toBe(2))

    context.event({ runId: 'run-0', type: 'text-delta', text: 'x', threadId: 'thread-1' })
    expect(openCalls).toBe(2)

    context.event({ runId: 'run-256', type: 'text-delta', text: 'x', threadId: 'thread-1' })
    await vi.waitFor(() => expect(openCalls).toBe(3))

    context.event({ runId: 'run-0', type: 'text-delta', text: 'x', threadId: 'thread-1' })
    await vi.waitFor(() => expect(openCalls).toBe(4))

    context.event({ runId: 'run-256', type: 'text-delta', text: 'x', threadId: 'thread-1' })
    expect(openCalls).toBe(4)
  })

  it('collapses a signal into one follow-up when a re-read is already in flight', async () => {
    const first = deferred()
    const second = deferred()
    let openCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command !== 'chat_thread_open') throw new Error(`unexpected command: ${command}`)
      openCalls += 1
      if (openCalls === 1) await first.promise
      else await second.promise
      return { entries: [], nextCursor: null }
    })
    const context = setup(invoke, { threadId: 'thread-1', summaries: [{ threadId: 'thread-1' }] })
    await context.start()

    const refreshing = context.controller.refreshOpenThread()
    await vi.waitFor(() => expect(openCalls).toBe(1))
    context.event({ runId: 'run-9', type: 'text-delta', text: 'ACP', threadId: 'thread-1' })
    context.event({ runId: 'run-10', type: 'text-delta', text: 'Two', threadId: 'thread-1' })
    expect(openCalls).toBe(1)
    first.resolve()
    await refreshing
    await vi.waitFor(() => expect(openCalls).toBe(2))
    second.resolve()
    await vi.waitFor(() => expect(openCalls).toBe(2))
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_thread_open')).toHaveLength(2)
  })

  it('forgets events held for another run once the submission returns', async () => {
    const first = deferred()
    const second = deferred()
    const invoke = vi.fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
    const context = setup(invoke)
    await context.start()

    const sending = context.controller.send()
    context.event({ runId: 'run-9', type: 'text-delta', text: 'Another surface' })
    first.resolve({ runId: 'run-1' })
    await sending
    context.setActive(null)
    context.setDraft('Second question')
    const resending = context.controller.send()
    second.resolve({ runId: 'run-9' })
    await resending

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-9', phase: 'thinking', text: '' })
  })

  it('forgets events held for a run whose submission fails', async () => {
    const first = deferred()
    const second = deferred()
    const invoke = vi.fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
    const context = setup(invoke)
    await context.start()

    const sending = context.controller.send()
    context.event({ runId: 'run-1', type: 'text-delta', text: 'Held' })
    first.reject('The message could not be sent.')
    await sending
    const resending = context.controller.send()
    second.resolve({ runId: 'run-1' })
    await resending

    expect(context.messages().at(-1).run).toMatchObject({ id: 'run-1', phase: 'thinking', text: '' })
  })

  it('reconciles a resume onto the newer live projection', async () => {
    const resume = deferred()
    const context = setup(vi.fn((command) => command === 'chat_resume' ? resume.promise : undefined))
    await context.start()
    const interrupted = { id: 'run-1', phase: 'interrupted', text: '', resumable: true }
    context.setMessages([{ role: 'assistant', run: interrupted }])
    const resuming = context.controller.resume(interrupted)
    context.event({ runId: 'run-1', type: 'prompt-accepted' })
    context.event({ runId: 'run-1', type: 'text-delta', text: 'Newer' })
    resume.resolve()
    await resuming

    expect(context.messages()[0].run).toMatchObject({
      phase: 'streaming',
      text: 'Newer',
      accepted: true,
    })
  })

  it('clears active when an event settles the current run', async () => {
    const summaries = [{ threadId: 'thread-1', title: 'Hello' }]
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_summaries') return { summaries, nextCursor: null }
      if (command === 'chat_current_thread') return 'thread-1'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke)
    await context.start()
    const run = { id: 'run-1', phase: 'streaming', text: 'Done' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)
    context.event({ runId: 'run-1', type: 'completed' })
    const messagePublishCount = context.onMessages.mock.calls.length

    expect(context.active()).toBeNull()
    await vi.waitFor(() => expect(context.onThreadSelected).toHaveBeenCalledWith('thread-1'))
    expect(invoke.mock.calls).toEqual([
      ['chat_thread_summaries', { limit: 20 }],
      ['chat_current_thread'],
    ])
    expect(context.onThreadSummaries).toHaveBeenCalledWith(summaries)
    expect(context.onFreshThread).toHaveBeenCalledWith(false)
    expect(context.onMessages).toHaveBeenCalledTimes(messagePublishCount)
    expect(context.onFocus).not.toHaveBeenCalled()
    expect(context.onFollow).not.toHaveBeenCalled()
  })

  it('leaves thread state unchanged when a settlement refresh fails', async () => {
    const invoke = vi.fn((command) => {
      if (command === 'chat_thread_summaries') return Promise.reject(new Error('offline'))
      if (command === 'chat_current_thread') return 'thread-2'
      throw new Error(`unexpected command: ${command}`)
    })
    const context = setup(invoke)
    await context.start()
    const run = { id: 'run-1', phase: 'streaming', text: 'Done' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)

    context.event({ runId: 'run-1', type: 'completed' })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2))

    expect(context.onThreadSummaries).not.toHaveBeenCalled()
    expect(context.onThreadSelected).not.toHaveBeenCalled()
    expect(context.onFreshThread).not.toHaveBeenCalled()
  })

  it('surfaces a failed submit only when it is the latest submission', async () => {
    const first = deferred()
    const second = deferred()
    const invoke = vi.fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)
    const context = setup(invoke)
    const older = context.controller.send()
    context.setActive(null)
    context.setDraft('Newer')
    const newer = context.controller.send()
    first.reject('stale failure')
    await older
    expect(context.errors).toEqual([])

    second.reject('latest failure')
    await newer
    expect(context.errors).toEqual(['latest failure'])
  })
})

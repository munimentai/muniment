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

function setup(invoke = vi.fn()) {
  let messages = []
  let active = null
  let announced = null
  let draft = 'Hello'
  let files = []
  let listener
  const errors = []
  const onMessages = vi.fn((next) => { messages = next })
  const onThreadSummaries = vi.fn()
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
    onMessages,
    onActive: (next) => { active = next },
    onAnnounce: (next) => { announced = next },
    onDraft: (next) => { draft = next },
    onFiles: (next) => { files = next },
    onSubmitError: (error) => { if (error) errors.push(error) },
    onCancelError: vi.fn(),
    onQueueError: vi.fn(),
    onHistoryError: vi.fn(),
    onThreadSummaries,
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
    errors,
    onMessages,
    onThreadSummaries,
    onThreadSelected,
    onFreshThread,
    onFocus,
    onFollow,
    setActive: (next) => { active = next },
    setDraft: (next) => { draft = next },
    setMessages: (next) => { messages = next },
  }
}

describe('chat controller', () => {
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
      invoke: vi.fn().mockRejectedValue(new Error('offline')),
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
    expect(onHistoryError).toHaveBeenLastCalledWith('The thread name could not be changed. Try again.')
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
      invoke: vi.fn().mockRejectedValue(new Error('offline')),
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
    expect(onHistoryError).toHaveBeenLastCalledWith('A new thread could not be started. Try again.')
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
    expect(onHistoryError).toHaveBeenLastCalledWith('Conversation history could not be restored. Try again.')
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

    expect(onHistoryError).toHaveBeenLastCalledWith('Conversation history could not be restored. Try again.')
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

  it('reconciles resume buffering onto the newer live projection', async () => {
    const resume = deferred()
    const context = setup(vi.fn((command) => command === 'chat_resume' ? resume.promise : undefined))
    await context.start()
    context.event({ runId: 'run-1', type: 'prompt-accepted' })
    const interrupted = { id: 'run-1', phase: 'interrupted', text: '', resumable: true }
    context.setMessages([{ role: 'assistant', run: interrupted }])
    const resuming = context.controller.resume(interrupted)
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

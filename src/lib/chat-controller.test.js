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
    onMessages: (next) => { messages = next },
    onActive: (next) => { active = next },
    onAnnounce: (next) => { announced = next },
    onDraft: (next) => { draft = next },
    onFiles: (next) => { files = next },
    onSubmitError: (error) => { if (error) errors.push(error) },
    onCancelError: vi.fn(),
    onQueueError: vi.fn(),
    onHistoryError: vi.fn(),
  })
  return {
    controller,
    start: () => controller.start(),
    event: (payload) => listener({ payload }),
    messages: () => messages,
    active: () => active,
    errors,
    setActive: (next) => { active = next },
    setDraft: (next) => { draft = next },
    setMessages: (next) => { messages = next },
  }
}

describe('chat controller', () => {
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
    const context = setup()
    await context.start()
    const run = { id: 'run-1', phase: 'streaming', text: 'Done' }
    context.setMessages([{ role: 'assistant', run }])
    context.setActive(run)
    context.event({ runId: 'run-1', type: 'completed' })

    expect(context.active()).toBeNull()
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

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createDictationController } from './dictation-controller.js'

function setup({ invoke, ...options } = {}) {
  let draft = ''
  const listeners = []
  const errors = []
  const states = []
  const controller = createDictationController({
    invoke: invoke ?? vi.fn(async (command) => {
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      return { state: 'stopped' }
    }),
    listen: vi.fn(async (_, callback) => {
      listeners.push(callback)
      return vi.fn()
    }),
    readDraft: () => draft,
    updateDraft: (next) => { draft = next },
    onState: (state) => states.push({ ...state }),
    onError: (message) => errors.push(message),
    ...options,
  })
  return {
    controller,
    listeners,
    errors,
    states,
    draft: () => draft,
    transcript(index, text) {
      listeners[index]({ payload: { type: 'transcript', text } })
    },
    partial(index, text) {
      listeners[index]({ payload: { type: 'partial', text } })
    },
  }
}

describe('dictation controller', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('allows another attempt when startup fails after a stop request', async () => {
    let starts = 0
    const invoke = vi.fn(async command => {
      if (command === 'dictation_start') { starts++; return { state: 'starting' } }
      if (command === 'dictation_stop') return { state: 'starting' }
      return { state: 'failed', message: 'The microphone did not start.' }
    })
    const context = setup({ invoke })
    await context.controller.start()
    await context.controller.stop()
    expect(context.controller.snapshot().finishing).toBe(true)
    await vi.advanceTimersByTimeAsync(100)
    expect(context.controller.busy()).toBe(false)
    expect(context.controller.snapshot().finishing).toBe(false)
    expect(context.errors).toContain('The microphone did not start.')
    await context.controller.start()
    expect(starts).toBe(2)
    context.controller.cleanup()
  })

  it('passes start options, cleans each transcript and hands the result to completion', async () => {
    const onComplete = vi.fn()
    const invoke = vi.fn(async (command) => ({ state: command === 'dictation_start' ? 'running' : 'stopped' }))
    const context = setup({ invoke, onComplete, transform: (text) => text.replace('um ', ''), startOptions: () => ({ mute: true }) })
    await context.controller.start()
    expect(invoke).toHaveBeenCalledWith('dictation_start', { options: { mute: true } })
    context.transcript(0, 'um hello')
    await context.controller.stop()
    await vi.advanceTimersByTimeAsync(100)
    expect(context.draft()).toBe('hello')
    expect(onComplete).toHaveBeenCalledWith('hello')
  })

  it('does not complete a cancelled dictation', async () => {
    const onComplete = vi.fn()
    const context = setup({ onComplete })
    await context.controller.start()
    context.transcript(0, 'hello')
    await context.controller.stop(true)
    await vi.advanceTimersByTimeAsync(100)
    expect(onComplete).not.toHaveBeenCalled()
  })

  it('ignores transcript events from a stale capture epoch', async () => {
    const context = setup()
    await context.controller.start()
    context.transcript(0, 'first')
    await context.controller.stop(true)
    await context.controller.start()
    context.transcript(0, 'stale')
    context.transcript(1, 'fresh')

    expect(context.draft()).toBe('fresh')
  })

  it('completes with the verbatim transcript after transcript silence', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
    })
    const context = setup({ invoke })
    await context.controller.start()
    context.transcript(0, 'one')
    await context.controller.stop()
    await vi.advanceTimersByTimeAsync(20)
    context.transcript(0, 'two')
    await vi.advanceTimersByTimeAsync(24)
    await vi.advanceTimersByTimeAsync(1)
    await Promise.resolve()

    expect(context.draft()).toBe('one two')
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      'dictation_start',
      'dictation_stop',
    ])
  })

  it('does not offer resident transforms after capture', async () => {
    const context = setup()
    await context.controller.start()
    context.transcript(0, 'captured')
    await context.controller.stop()
    await vi.advanceTimersByTimeAsync(26)
    expect(context.controller.snapshot()).not.toHaveProperty('eligible')
  })

  it.each([
    ['dictation_start', 'microphone denied'],
    ['dictation_stop', 'stop failed'],
  ])('surfaces a failed %s through the error callback', async (failedCommand, message) => {
    const invoke = vi.fn(async (command) => {
      if (command === failedCommand) throw message
      return { state: 'running' }
    })
    const context = setup({ invoke })
    await context.controller.start()
    if (failedCommand === 'dictation_stop') await context.controller.stop()

    expect(context.errors).toContain(message)
  })

  it('shows partial text after the draft and replaces it with the final transcript', async () => {
    const context = setup()
    await context.controller.start()
    context.partial(0, 'hello')
    expect(context.draft()).toBe('hello')
    expect(context.controller.snapshot()).toMatchObject({ partial: 'hello', pending: 5, transcript: '' })
    context.partial(0, 'hello wor')
    expect(context.draft()).toBe('hello wor')
    context.transcript(0, 'Hello world.')
    expect(context.draft()).toBe('Hello world.')
    expect(context.controller.snapshot()).toMatchObject({ partial: '', pending: 0, transcript: 'Hello world.' })
    context.partial(0, 'and')
    expect(context.draft()).toBe('Hello world. and')
    // The joining space counts with the partial text it introduces.
    expect(context.controller.snapshot().pending).toBe(4)
    context.partial(0, '')
    expect(context.draft()).toBe('Hello world.')
    expect(context.states.at(-1)).toMatchObject({ partial: '', pending: 0 })
  })

  it('keeps the text typed before dictation ahead of the partial text', async () => {
    let draft = 'Note:'
    const context = setup({ readDraft: () => draft, updateDraft: (next) => { draft = next } })
    await context.controller.start()
    context.partial(0, 'one')
    expect(draft).toBe('Note: one')
    expect(context.controller.snapshot().pending).toBe(4)
  })

  it('cleans partial text like final text', async () => {
    const context = setup({ transform: (text) => text.replace('um ', '') })
    await context.controller.start()
    context.partial(0, 'um hello')
    expect(context.draft()).toBe('hello')
  })

  it('completes with the final text only and drops partial text no final replaced', async () => {
    const onComplete = vi.fn()
    const context = setup({ onComplete })
    await context.controller.start()
    context.transcript(0, 'one')
    context.partial(0, 'two')
    await context.controller.stop()
    await vi.advanceTimersByTimeAsync(100)
    expect(context.draft()).toBe('one')
    expect(onComplete).toHaveBeenCalledWith('one')
    expect(context.controller.snapshot().pending).toBe(0)
  })

  it('sends the final text when the last partial turns final at stop', async () => {
    const onComplete = vi.fn()
    const context = setup({ onComplete })
    await context.controller.start()
    context.partial(0, 'send th')
    await context.controller.stop()
    context.transcript(0, 'Send this.')
    await vi.advanceTimersByTimeAsync(100)
    expect(context.draft()).toBe('Send this.')
    expect(onComplete).toHaveBeenCalledWith('Send this.')
  })

  it('restores the draft on cancel with partial text showing', async () => {
    let draft = 'Before'
    const context = setup({ readDraft: () => draft, updateDraft: (next) => { draft = next } })
    await context.controller.start()
    context.partial(0, 'never mind')
    await context.controller.stop(true)
    context.partial(0, 'late')
    expect(draft).toBe('Before')
    expect(context.controller.snapshot()).toMatchObject({ partial: '', pending: 0 })
  })

  it('drops partial text when the session fails', async () => {
    let calls = 0
    const invoke = vi.fn(async (command) => {
      if (command === 'dictation_start') return { state: 'running' }
      calls += 1
      return { state: 'failed', message: 'Speech recognition failed.' }
    })
    const context = setup({ invoke })
    await context.controller.start()
    context.partial(0, 'lost')
    await vi.advanceTimersByTimeAsync(100)
    expect(calls).toBe(1)
    expect(context.draft()).toBe('')
    expect(context.errors).toContain('Speech recognition failed.')
  })
})

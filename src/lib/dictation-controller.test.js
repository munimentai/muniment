import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createDictationController } from './dictation-controller.js'

function setup({ invoke } = {}) {
  let draft = ''
  const listeners = []
  const errors = []
  const states = []
  const controller = createDictationController({
    invoke: invoke ?? vi.fn(async (command) => {
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return 'Polished'
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
  }
}

describe('dictation controller', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
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

  it('completes and polishes once after transcript silence', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return 'Polished'
    })
    const context = setup({ invoke })
    await context.controller.start()
    context.transcript(0, 'one')
    await context.controller.stop()
    await vi.advanceTimersByTimeAsync(20)
    context.transcript(0, 'two')
    await vi.advanceTimersByTimeAsync(24)
    expect(invoke).not.toHaveBeenCalledWith('dictation_polish', expect.anything())
    await vi.advanceTimersByTimeAsync(1)
    await Promise.resolve()

    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_polish')).toEqual([
      ['dictation_polish', { transcript: 'one two' }],
    ])
  })

  it('expires a transform offer after six seconds and invalidates it on a new capture', async () => {
    const context = setup()
    await context.controller.start()
    context.transcript(0, 'captured')
    await context.controller.stop()
    await vi.advanceTimersByTimeAsync(26)
    expect(context.controller.snapshot().eligible).toMatchObject({ segment: 'Polished' })

    await vi.advanceTimersByTimeAsync(5998)
    expect(context.controller.snapshot().eligible).not.toBeNull()
    await vi.advanceTimersByTimeAsync(2)
    expect(context.controller.snapshot().eligible).toBeNull()

    await context.controller.start()
    context.transcript(1, 'again')
    await context.controller.stop()
    await vi.advanceTimersByTimeAsync(26)
    expect(context.controller.snapshot().eligible).not.toBeNull()
    await context.controller.start()
    expect(context.controller.snapshot().eligible).toBeNull()
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
})

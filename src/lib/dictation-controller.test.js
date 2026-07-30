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
})

import { describe, expect, it } from 'vitest'
import { applyBufferedChatEvents, applyChatEvent, receiptParts, shouldSend } from './chat-state.js'

describe('chat composer and projection', () => {
  it('sends Enter, retains Shift+Enter, and blocks duplicate submits', () => {
    expect(shouldSend({ key: 'Enter', shiftKey: false, isComposing: false }, 'hello', false)).toBe(true)
    expect(shouldSend({ key: 'Enter', shiftKey: true, isComposing: false }, 'hello', false)).toBe(false)
    expect(shouldSend({ key: 'Enter', shiftKey: false, isComposing: false }, 'hello', true)).toBe(false)
  })

  it('moves thinking to streaming and removes signal on every terminal event', () => {
    let run = { id: 'r', phase: 'thinking', text: '' }
    run = applyChatEvent(run, { runId: 'r', type: 'text-delta', text: 'Hi' })
    expect(run).toMatchObject({ phase: 'streaming', text: 'Hi' })
    expect(applyChatEvent(run, { runId: 'r', type: 'completed' }).phase).toBe('complete')
    expect(applyChatEvent(run, { runId: 'r', type: 'failed' }).phase).toBe('failed')
  })

  it('does not invent missing receipt values', () => {
    expect(receiptParts({ route: 'fast', capabilities: [{ name: 'search', version: '2' }] }))
      .toEqual(['fast', 'search@2'])
  })

  it('preserves all projections that arrive before submit resolves', () => {
    const run = applyBufferedChatEvents({ id: 'r', phase: 'thinking', text: '' }, [
      { runId: 'r', phase: 'thinking', text: '' },
      { runId: 'r', phase: 'streaming', text: 'fast ' },
      { runId: 'r', phase: 'streaming', text: 'fast reply' },
      { runId: 'r', phase: 'complete', text: 'fast reply', receipt: { route: 'fast' } },
    ])
    expect(run).toMatchObject({ phase: 'complete', text: 'fast reply', receipt: { route: 'fast' } })
  })
})

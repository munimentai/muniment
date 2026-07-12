import { describe, expect, it } from 'vitest'
import { applyBufferedChatEvents, applyChatEvent, composerAction, receiptParts } from './chat-state.js'

describe('chat composer and projection', () => {
  it('chooses submit or steer from the active run', () => {
    const enter = { key: 'Enter', shiftKey: false, isComposing: false }
    expect(composerAction(enter, 'hello', null)).toBe('submit')
    expect(composerAction(enter, 'hello', { id: 'run-1' })).toBe('steer')
    expect(composerAction(enter, 'hello', { id: 'pending' })).toBeNull()
  })

  it('blocks modified Enter, IME composition, and empty drafts', () => {
    expect(composerAction({ key: 'Enter', shiftKey: true, isComposing: false }, 'hello', null)).toBeNull()
    expect(composerAction({ key: 'Enter', shiftKey: false, isComposing: true }, 'hello', null)).toBeNull()
    expect(composerAction({ key: 'Enter', shiftKey: false, isComposing: false }, '  ', null)).toBeNull()
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

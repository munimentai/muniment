import { describe, expect, it } from 'vitest'
import { applyBufferedChatEvents, applyChatEvent, composerAction, receiptParts, receiptRows, toolName, toolStatus } from './chat-state.js'

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

  it('threads projected tool activity into the active run', () => {
    const toolActivity = [
      { effectId: 'tool-1', displayName: 'Read file', status: 'running' },
      { effectId: 'tool-2', displayName: null, status: 'completed' },
    ]
    expect(applyChatEvent(
      { id: 'r', phase: 'thinking', text: '' },
      { runId: 'r', phase: 'streaming', text: 'Working', toolActivity },
    )).toMatchObject({ phase: 'streaming', text: 'Working', toolActivity })
  })

  it('gives tools accessible neutral names and explicit statuses', () => {
    expect(toolName({ displayName: '  ' })).toBe('Tool activity')
    expect(toolName({ displayName: 'Search files' })).toBe('Search files')
    expect(toolStatus({ status: 'failed' })).toBe('failed')
  })

  it('does not invent missing receipt values', () => {
    expect(receiptParts({ route: 'fast', capabilities: [{ name: 'search', version: '2' }] }))
      .toEqual(['fast', 'search@2'])
  })

  it('projects every present receipt field in record order', () => {
    expect(receiptRows({
      route: 'fast', model: 'glm-5.2', cost: '$0.04', time: '1.8s',
      capabilities: [{ name: 'search', version: '2' }, { name: 'files', version: '1' }],
    })).toEqual([
      { label: 'Route', value: 'fast', route: true },
      { label: 'Model', value: 'glm-5.2', route: false },
      { label: 'Cost', value: '$0.04', route: false },
      { label: 'Time', value: '1.8s', route: false },
      { label: 'Capability', value: 'search@2', route: false },
      { label: 'Capability', value: 'files@1', route: false },
    ])
  })

  it('omits absent receipt fields', () => {
    expect(receiptRows({ model: 'glm-5.2', time: '1.8s' })).toEqual([
      { label: 'Model', value: 'glm-5.2', route: false },
      { label: 'Time', value: '1.8s', route: false },
    ])
  })

  it('projects capabilities without inventing other rows', () => {
    expect(receiptRows({ capabilities: [{ name: 'search', version: '2' }] }))
      .toEqual([{ label: 'Capability', value: 'search@2', route: false }])
  })

  it('projects no rows from an empty receipt', () => {
    expect(receiptRows({})).toEqual([])
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

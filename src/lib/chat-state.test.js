import { describe, expect, it } from 'vitest'
import { applyBufferedChatEvents, applyChatEvent, composerAction, receiptParts, receiptRows, runAnnouncement, toolName, toolStatus } from './chat-state.js'

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

  it('announces one coarse in-progress state for a whole generation', () => {
    expect(runAnnouncement({ phase: 'thinking', text: '' })).toBe('Generating a reply.')
    expect(runAnnouncement({ phase: 'streaming', text: 'Half an ans' })).toBe('Generating a reply.')
    expect(runAnnouncement({ phase: 'resuming', text: 'Partial answer' })).toBe('Resuming the interrupted reply.')
  })

  it('keeps the announcement identical across every streamed chunk and pause of a run', () => {
    // src-tauri/src/chat.rs projection_phase also emits pending-permission mid-run.
    const phases = [
      { phase: 'thinking', text: '' },
      { phase: 'streaming', text: 'A' },
      { phase: 'pending-permission', text: 'A rout' },
      { phase: 'streaming', text: 'A routed answer' },
    ].map(runAnnouncement)
    expect(new Set(phases)).toEqual(new Set(['Generating a reply.']))
  })

  it('announces the finished reply once with its text', () => {
    expect(runAnnouncement({ phase: 'complete', text: 'A routed answer' })).toBe('Reply complete. A routed answer')
    expect(runAnnouncement({ phase: 'complete', text: '' })).toBe('Reply complete.')
    expect(runAnnouncement({ phase: 'complete' })).toBe('Reply complete.')
  })

  it('announces every terminal outcome the run can reach', () => {
    expect(runAnnouncement({ phase: 'failed', text: '' })).toBe('Reply failed.')
    expect(runAnnouncement({ phase: 'interrupted', text: 'Partial answer' })).toBe('Reply interrupted.')
    expect(runAnnouncement({ phase: 'cancelled', text: 'Partial answer' })).toBe('Reply stopped.')
  })

  it('says nothing without a run or for an unrecognized phase', () => {
    expect(runAnnouncement(null)).toBe('')
    expect(runAnnouncement(undefined)).toBe('')
    expect(runAnnouncement({ phase: 'a-phase-that-does-not-exist', text: 'Partial answer' })).toBe('')
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

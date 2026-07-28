import { describe, expect, it } from 'vitest'
import { applyBufferedChatEvents, applyChatEvent, composerAction, historyMessages, permissionGateAction, receiptLabel, receiptRows, receiptSummary, runAnnouncement, toolName, toolStatus } from './chat-state.js'

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

  it('commits permission fields with the key for their gate kind', () => {
    expect(permissionGateAction({ key: 'Enter' }, 'input', 'Linux x86_64')).toBe('commit')
    expect(permissionGateAction({ key: 'Enter', metaKey: true }, 'editor', 'MacIntel')).toBe('commit')
    expect(permissionGateAction({ key: 'Enter', ctrlKey: true }, 'editor', 'Linux x86_64')).toBe('commit')
  })

  it('ignores permission field keys that do not commit', () => {
    expect(permissionGateAction({ key: 'Enter' }, 'editor', 'Linux x86_64')).toBeNull()
    expect(permissionGateAction({ key: 'Enter', shiftKey: true }, 'input', 'Linux x86_64')).toBeNull()
    expect(permissionGateAction({ key: 'Enter', isComposing: true }, 'input', 'Linux x86_64')).toBeNull()
    expect(permissionGateAction({ key: 'Escape' }, 'input', 'Linux x86_64')).toBeNull()
    expect(permissionGateAction({ key: 'Enter', ctrlKey: true, metaKey: true }, 'editor', 'MacIntel')).toBeNull()
    expect(permissionGateAction({ key: 'Enter', ctrlKey: true, altKey: true }, 'editor', 'Linux x86_64')).toBeNull()
    expect(permissionGateAction({ key: 'Enter', ctrlKey: true }, 'input', 'Linux x86_64')).toBeNull()
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

  it('sets and clears a projected permission gate', () => {
    const gate = { gateId: 'gate-1', kind: 'confirm', title: 'Run rm?', message: '/tmp/draft' }
    const waiting = applyChatEvent(
      { id: 'r', phase: 'thinking', text: '' },
      { runId: 'r', phase: 'pending-permission', text: '', pendingPermission: gate },
    )
    expect(waiting.pendingPermission).toEqual(gate)
    expect(applyChatEvent(waiting, { runId: 'r', phase: 'streaming', text: 'Allowed' }).pendingPermission).toBeNull()
  })

  it('carries and clears permission gates across buffered events', () => {
    const gate = { gateId: 'gate-1', kind: 'select', title: 'Choose access', options: ['Once'] }
    const waiting = applyBufferedChatEvents({ id: 'r', phase: 'thinking', text: '' }, [
      { runId: 'r', phase: 'pending-permission', text: '', pendingPermission: gate },
    ])
    expect(waiting.pendingPermission).toEqual(gate)
    expect(applyBufferedChatEvents(waiting, [
      { runId: 'r', phase: 'streaming', text: 'Continuing' },
    ]).pendingPermission).toBeNull()
  })

  it('restores a permission gate from history', () => {
    const gate = { gateId: 'gate-1', kind: 'input', title: 'Enter a value' }
    const [, assistant] = historyMessages([{
      runId: 'r', prompt: 'Help', phase: 'pending-permission', text: '', pendingPermission: gate,
    }])
    expect(assistant.run.pendingPermission).toEqual(gate)
    expect(historyMessages([{ runId: 'r', phase: 'streaming', text: 'Hi' }])[0].run.pendingPermission).toBeNull()
  })

  it('gives tools accessible neutral names and explicit statuses', () => {
    expect(toolName({ displayName: '  ' })).toBe('Tool activity')
    expect(toolName({ displayName: 'Search files' })).toBe('Search files')
    expect(toolStatus({ status: 'failed' })).toBe('failed')
  })

  it('summarizes a full receipt as route → model · cost · time', () => {
    expect(receiptSummary({
      route: 'analysis/high', model: 'glm-5.2', cost: '$0.0041', time: '3.8s',
      capabilities: [{ name: 'search', version: '2' }],
    })).toEqual({ route: 'analysis/high', separator: ' → ', detail: 'glm-5.2 · $0.0041 · 3.8s · search@2' })
  })

  it('keeps the arrow for the route→model relation alone', () => {
    // No model to point at: the route joins the rest with the plain separator.
    expect(receiptSummary({ route: 'analysis/high', cost: '$0.0041' }))
      .toEqual({ route: 'analysis/high', separator: ' · ', detail: '$0.0041' })
    expect(receiptSummary({ route: 'analysis/high', model: 'glm-5.2' }))
      .toEqual({ route: 'analysis/high', separator: ' → ', detail: 'glm-5.2' })
  })

  it('names the route field so signal never lands on another segment', () => {
    // design-spec §1.2: --signal is the route segment's alone.
    expect(receiptSummary({ model: 'glm-5.2', cost: '$0.0041' }))
      .toEqual({ route: null, separator: '', detail: 'glm-5.2 · $0.0041' })
    expect(receiptSummary({ capabilities: [{ name: 'search', version: '2' }] }))
      .toEqual({ route: null, separator: '', detail: 'search@2' })
  })

  it('summarizes partial receipts without a stray separator or dangling arrow', () => {
    expect(receiptSummary({ route: 'analysis/high' }))
      .toEqual({ route: 'analysis/high', separator: '', detail: '' })
    expect(receiptSummary({})).toEqual({ route: null, separator: '', detail: '' })
    expect(receiptSummary(null)).toEqual({ route: null, separator: '', detail: '' })
    expect(receiptSummary({ route: '', model: 'glm-5.2', cost: '', time: '3.8s' }))
      .toEqual({ route: null, separator: '', detail: 'glm-5.2 · 3.8s' })
    expect(receiptSummary({ route: 'free', model: 'glm-5.2', cost: 0 }))
      .toEqual({ route: 'free', separator: ' → ', detail: 'glm-5.2 · 0' })
  })

  it('does not invent missing receipt values', () => {
    expect(receiptSummary({ route: 'fast', capabilities: [{ name: 'search' }, { version: '2' }, null] }))
      .toEqual({ route: 'fast', separator: '', detail: '' })
  })

  it('states the route-to-model relation in words for screen readers', () => {
    expect(receiptLabel({
      route: 'analysis/high', model: 'glm-5.2', cost: '$0.0041', time: '3.8s',
      capabilities: [{ name: 'search', version: '2' }],
    })).toBe('Routed via analysis/high to model glm-5.2, $0.0041, 3.8s, search@2')
  })

  it('labels partial receipts without naming a field the receipt lacks', () => {
    expect(receiptLabel({ route: 'analysis/high', time: '3.8s' })).toBe('Routed via analysis/high, 3.8s')
    expect(receiptLabel({ model: 'glm-5.2' })).toBe('Model glm-5.2')
    expect(receiptLabel({ cost: '$0.0041' })).toBe('$0.0041')
    expect(receiptLabel({})).toBe('')
    expect(receiptLabel(null)).toBe('')
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

  it('announces a permission pause between streamed chunks', () => {
    // src-tauri/src/chat.rs projection_phase also emits pending-permission mid-run.
    const phases = [
      { phase: 'thinking', text: '' },
      { phase: 'streaming', text: 'A' },
      { phase: 'pending-permission', text: 'A rout' },
      { phase: 'streaming', text: 'A routed answer' },
    ].map(runAnnouncement)
    expect(phases).toEqual([
      'Generating a reply.',
      'Generating a reply.',
      'Waiting for your decision.',
      'Generating a reply.',
    ])
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

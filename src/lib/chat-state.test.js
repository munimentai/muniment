import { describe, expect, it } from 'vitest'
import { applyBufferedChatEvents, applyChatEvent, codeDiffPermissionAnswer, composerAction, historyMessages, permissionGateAction, permissionGateCommitHint, receiptLabel, receiptRows, receiptSummary, runAnnouncement, settledPhases, toolName, toolStatus, unsettledRun } from './chat-state.js'

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

  it('names the editor commit chord only for editor requests', () => {
    expect(permissionGateCommitHint('editor', 'MacIntel')).toBe('⌘⏎ submits')
    expect(permissionGateCommitHint('editor', 'Linux x86_64')).toBe('Ctrl ⏎ submits')
    expect(permissionGateCommitHint('confirm', 'MacIntel')).toBeNull()
    expect(permissionGateCommitHint('select', 'MacIntel')).toBeNull()
    expect(permissionGateCommitHint('input', 'MacIntel')).toBeNull()
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

  it('binds a code diff answer to every proposal identifier', () => {
    expect(codeDiffPermissionAnswer({
      gateId: 'gate-1',
      effect_id: 'effect-1',
      code_diff_id: 'diff-1',
      diff_sha256: 'diff-hash',
      write_plan_sha256: 'plan-hash',
    })).toEqual({
      type: 'codeDiff',
      value: {
        gate_id: 'gate-1',
        effect_id: 'effect-1',
        code_diff_id: 'diff-1',
        diff_sha256: 'diff-hash',
        write_plan_sha256: 'plan-hash',
      },
    })
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

  it('projects one memory row per recall before capabilities', () => {
    expect(receiptRows(
      { route: 'fast', model: 'glm-5.2', cost: '$0.04', time: '1.8s', capabilities: [{ name: 'files', version: '1' }] },
      [
        { query: 'lease', files: ['/Documents/Muniment/lease.pdf', '/Documents/Muniment/notes.md'] },
        { query: 'missing clause', files: [] },
      ],
    )).toEqual([
      { label: 'Route', value: 'fast', route: true },
      { label: 'Model', value: 'glm-5.2', route: false },
      { label: 'Cost', value: '$0.04', route: false },
      { label: 'Time', value: '1.8s', route: false },
      { label: 'Memory', value: 'lease · 2 files', files: ['/Documents/Muniment/lease.pdf', '/Documents/Muniment/notes.md'], route: false },
      { label: 'Memory', value: 'missing clause · 0 files', files: [], route: false },
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

  it('carries recalls from live events and restored history', () => {
    const recalls = [{ query: 'lease', files: ['/Documents/Muniment/lease.pdf'] }]
    expect(applyChatEvent(
      { id: 'r', phase: 'thinking', text: '' },
      { runId: 'r', phase: 'complete', text: 'Done', recalls },
    ).recalls).toEqual(recalls)
    expect(applyChatEvent(
      { id: 'r', phase: 'thinking', text: '' },
      { runId: 'r', phase: 'complete', text: 'Done' },
    ).recalls).toEqual([])
    expect(historyMessages([{ runId: 'r', phase: 'complete', text: 'Done', recalls }])[0].run.recalls).toEqual(recalls)
    expect(historyMessages([{ runId: 'r', phase: 'complete', text: 'Done' }])[0].run.recalls).toEqual([])
  })

  it('carries applied diffs from live events and restored history', () => {
    const appliedDiffs = [{ effectId: 'effect-1', codeDiffId: 'diff-1', diff: null }]
    expect(applyChatEvent(
      { id: 'r', phase: 'thinking', text: '' },
      { runId: 'r', type: 'completed', appliedDiffs },
    ).appliedDiffs).toEqual(appliedDiffs)
    expect(applyChatEvent(
      { id: 'r', phase: 'thinking', text: '' },
      { runId: 'r', phase: 'complete', text: 'Done' },
    ).appliedDiffs).toEqual([])
    expect(historyMessages([{ runId: 'r', phase: 'complete', text: 'Done', appliedDiffs }])[0].run.appliedDiffs).toEqual(appliedDiffs)
    expect(historyMessages([{ runId: 'r', phase: 'complete', text: 'Done' }])[0].run.appliedDiffs).toEqual([])
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

  it('holds every phase a stopped run reaches', () => {
    expect([...settledPhases].sort()).toEqual(['cancelled', 'complete', 'failed', 'interrupted'])
  })

  it('names no unsettled run in an empty or fully settled list', () => {
    expect(unsettledRun([])).toBeNull()
    expect(unsettledRun()).toBeNull()
    expect(unsettledRun(historyMessages([
      { runId: 'r1', prompt: 'One', phase: 'complete', text: 'Done' },
      { runId: 'r2', prompt: 'Two', phase: 'cancelled', text: 'Stopped' },
      { runId: 'r3', prompt: 'Three', phase: 'failed', text: '' },
      { runId: 'r4', prompt: 'Four', phase: 'interrupted', text: 'Partial' },
    ]))).toBeNull()
  })

  it('names the run the runtime still drives past user messages and settled runs', () => {
    const messages = historyMessages([
      { runId: 'r1', prompt: 'One', phase: 'complete', text: 'Done' },
      { runId: 'r2', prompt: 'Two', phase: 'streaming', text: 'Half an ans' },
    ])
    expect(messages.filter((message) => message.role === 'user')).toHaveLength(2)
    expect(unsettledRun(messages)).toMatchObject({ id: 'r2', phase: 'streaming', text: 'Half an ans' })
  })

  it('takes the newest run where several stayed unsettled', () => {
    expect(unsettledRun(historyMessages([
      { runId: 'r1', prompt: 'One', phase: 'thinking', text: '' },
      { runId: 'r2', prompt: 'Two', phase: 'pending-permission', text: 'A rout' },
      { runId: 'r3', prompt: 'Three', phase: 'complete', text: 'Done' },
    ]))).toMatchObject({ id: 'r2', phase: 'pending-permission' })
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

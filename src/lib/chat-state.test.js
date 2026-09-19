import { describe, expect, it } from 'vitest'
import { applyBufferedChatEvents, applyChatEvent, codeDiffPermissionAnswer, composerAction, historyMessages, modelLabel, permissionGateAction, permissionGateCommitHint, receiptLabel, receiptRows, receiptSummary, receiptUsageColumns, runAnnouncement, runFailureMessage, runStage, settledPhases, stageWord, toolName, toolStatus, toolVerb, unsettledRun } from './chat-state.js'

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
    expect(permissionGateCommitHint('editor', 'MacIntel')).toBe('⌘ ⏎ submits')
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

  it('keeps the prompt storage notice across live events and restored history', () => {
    const promptStorageNotice = 'Prompt text stays in runtime memory for this run. Keyring error -25307: A default keychain could not be found.'
    const entry = { runId: 'r', phase: 'streaming', text: 'Reply', promptStorageNotice }
    const initial = { id: 'r', phase: 'thinking', text: '' }
    const live = applyChatEvent(initial, entry)
    expect(live.promptStorageNotice).toBe(promptStorageNotice)
    expect(applyChatEvent(live, { runId: 'r', phase: 'complete', text: 'Reply' }).promptStorageNotice).toBe(promptStorageNotice)
    expect(historyMessages([{ ...entry, prompt: null }])[0].run.promptStorageNotice).toBe(promptStorageNotice)
    expect(applyChatEvent(initial, { ...entry, runId: 'other' })).toBe(initial)
  })

  it('tracks the in-flight word from the projections: Routing, Thinking, the tool verb, Writing', () => {
    let run = { id: 'r', phase: 'thinking', text: '' }
    expect(stageWord(run.stage)).toBe('Routing')
    run = applyChatEvent(run, { runId: 'r', phase: 'thinking', text: '', toolActivity: [] })
    expect(stageWord(run.stage)).toBe('Routing')
    run = applyChatEvent(run, { runId: 'r', phase: 'thinking', text: '', turnStarted: true, toolActivity: [] })
    expect(stageWord(run.stage)).toBe('Thinking')
    run = applyChatEvent(run, { runId: 'r', phase: 'thinking', text: '', turnStarted: true, toolActivity: [{ effectId: 'e1', displayName: 'bash', status: 'running' }] })
    expect(stageWord(run.stage)).toBe('Running')
    run = applyChatEvent(run, { runId: 'r', phase: 'thinking', text: '', turnStarted: true, toolActivity: [{ effectId: 'e1', displayName: 'bash', status: 'completed' }] })
    expect(stageWord(run.stage)).toBe('Thinking')
    run = applyChatEvent(run, { runId: 'r', phase: 'streaming', text: 'Hi', turnStarted: true, toolActivity: [{ effectId: 'e1', displayName: 'bash', status: 'completed' }] })
    expect(stageWord(run.stage)).toBe('Writing')
    run = applyChatEvent(run, { runId: 'r', phase: 'streaming', text: 'Hi', turnStarted: true, toolActivity: [{ effectId: 'e1', displayName: 'bash', status: 'completed' }, { effectId: 'e2', displayName: 'mcp__linear__list_issues', status: 'running' }] })
    expect(stageWord(run.stage)).toBe('Calling linear')
    expect(runStage(null, { phase: 'streaming', text: 'Restored', toolActivity: [] })).toBe('writing')
    expect(historyMessages([{ runId: 'r', phase: 'thinking', text: '', turnStarted: true, toolActivity: [] }])[0].run.stage).toBe('thinking')
  })

  it('names every tool by a plain verb and never by a vendor, a model or a harness', () => {
    for (const tool of ['read', 'grep', 'find', 'ls']) expect(toolVerb(tool)).toBe('Reading')
    for (const tool of ['write', 'edit']) expect(toolVerb(tool)).toBe('Editing')
    for (const tool of ['bash', 'powershell']) expect(toolVerb(tool)).toBe('Running')
    for (const tool of ['web_search', 'fetch_content']) expect(toolVerb(tool)).toBe('Searching')
    expect(toolVerb('subagent')).toBe('Delegating')
    for (const tool of ['bg_run', 'bg_status', 'bg_logs', 'bg_kill']) expect(toolVerb(tool)).toBe('Working in the background')
    expect(toolVerb('mcp')).toBe('Calling a server')
    expect(toolVerb('mcp__github__search')).toBe('Calling github')
    expect(toolVerb('unknown_tool')).toBe('Working')
    expect(stageWord(undefined)).toBe('Routing')
    for (const word of ['Routing', 'Thinking', 'Writing', toolVerb('subagent')]) expect(word).not.toMatch(/pi|muniment|claude|openai|gpt/i)
  })

  it('moves thinking to streaming and removes signal on every terminal event', () => {
    let run = { id: 'r', phase: 'thinking', text: '' }
    run = applyChatEvent(run, { runId: 'r', type: 'text-delta', text: 'Hi' })
    expect(run).toMatchObject({ phase: 'streaming', text: 'Hi' })
    expect(applyChatEvent(run, { runId: 'r', type: 'completed' }).phase).toBe('complete')
    expect(applyChatEvent(run, { runId: 'r', type: 'failed' }).phase).toBe('failed')
  })

  it.each([
    ['No reply arrived within 30 seconds. Try again.', 'No reply arrived within 30 seconds.'],
    ['Acme Inc. logo.png exceeds the 10 MB image limit.', 'Acme Inc. logo.png exceeds the 10 MB image limit.'],
    ['Acme Inc. logo.png exceeds the 10 MB image limit. Choose a smaller image before sending again.', 'Acme Inc. logo.png exceeds the 10 MB image limit.'],
  ])('keeps the recorded cause %j across live and restored runs', (failureReason, message) => {
    const entry = { runId: 'r', phase: 'failed', text: 'Partial answer', failureReason }
    const initial = { id: 'r', phase: 'streaming', text: 'Partial answer' }
    const projections = [
      applyChatEvent(initial, entry),
      applyBufferedChatEvents(initial, [entry]),
      applyChatEvent(initial, { runId: 'r', type: 'failed', failureReason }),
      historyMessages([entry])[0].run,
    ]
    for (const run of projections) {
      expect(run).toMatchObject({ phase: 'failed', text: 'Partial answer', failureReason })
      expect(runFailureMessage(run)).toBe(message)
      expect(runAnnouncement(run)).toBe(message)
    }
    expect(applyChatEvent(initial, { ...entry, runId: 'other' })).toBe(initial)
    expect(applyChatEvent(projections[0], { runId: 'r', phase: 'thinking' }).failureReason).toBeNull()
  })

  it.each([undefined, null, '', '  \n ', 0, {}, 'Try again.'])(
    'uses the fallback for an absent or invalid cause: %j', (failureReason) => {
      expect(runFailureMessage({ failureReason })).toBe('Reply failed.')
    },
  )

  it('keeps cause punctuation and folds line breaks without changing addresses or decimals', () => {
    expect(runFailureMessage({ failureReason: '  Cannot reach\nhttps://example.com after 1.5 seconds  ' }))
      .toBe('Cannot reach https://example.com after 1.5 seconds.')
    expect(runFailureMessage({ failureReason: 'The provider refused access!' })).toBe('The provider refused access!')
    expect(runFailureMessage({ failureReason: 'The provider refused access. Check the key and try again.' }))
      .toBe('The provider refused access.')
  })

  it.each([
    'Try again.',
    'Check the key and try again.',
    'Check the files and try again.',
    'Choose a smaller image before sending again.',
    'Remove an image before sending again.',
    'Remove images or choose smaller images before sending again.',
    'Choose a PNG, JPEG, GIF, or WebP image before sending again.',
  ])('removes only the trailing retry guidance %j', (guidance) => {
    const cause = 'Acme Inc. logo.png exceeds the 10 MB image limit.'
    expect(runFailureMessage({ failureReason: `${cause} ${guidance}` })).toBe(cause)
    expect(runFailureMessage({ failureReason: `${cause} ${guidance.slice(0, -1)}` })).toBe(cause)
  })

  it.each([
    'Acme! logo.png exceeds the 10 MB image limit.',
    'Acme? logo.png exceeds the 10 MB image limit.',
    'Try again.png exceeds the 10 MB image limit.',
    'Acme Inc. logo.png failed. The provider refused the image.',
  ])('preserves punctuation and all recorded cause text in %j', (failureReason) => {
    expect(runFailureMessage({ failureReason })).toBe(failureReason)
    expect(runAnnouncement({ phase: 'failed', failureReason })).toBe(failureReason)
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

  it('summarizes a receipt as the route, the model as words, and the time', () => {
    expect(receiptSummary({
      route: 'analysis/high', model: 'glm-5.2', cost: '$0.0041', time: '3.8s',
      capabilities: [{ name: 'search', version: '2' }],
      tools: [{ name: 'read', calls: 2, failed: 0 }],
    })).toEqual({ route: 'analysis/high', model: 'glm 5.2', time: '4s' })
    expect(receiptSummary({ model: 'openai-codex/gpt-5.5', time: '12.6s' })).toEqual({ route: null, model: 'openai codex/gpt 5.5', time: '13s' })
    expect(modelLabel('hf.co/lmstudio-community/Qwen3.5-4B-GGUF:Q4_K_M')).toBe('hf.co/lmstudio community/Qwen3.5 4B GGUF:Q4_K_M')
    expect(modelLabel('')).toBe(null)
  })

  it('names the route field so signal never lands on another segment', () => {
    // design-spec §1.2: --signal is the route segment's alone.
    expect(receiptSummary({ model: 'glm-5.2', cost: '$0.0041' })).toEqual({ route: null, model: 'glm 5.2', time: null })
    expect(receiptSummary({ route: '', model: 'glm-5.2', cost: '', time: '3.8s' })).toEqual({ route: null, model: 'glm 5.2', time: '4s' })
  })

  it('summarizes partial receipts without inventing a field', () => {
    expect(receiptSummary({ route: 'analysis/high' })).toEqual({ route: 'analysis/high', model: null, time: null })
    expect(receiptSummary({})).toEqual({ route: null, model: null, time: null })
    expect(receiptSummary(null)).toEqual({ route: null, model: null, time: null })
    expect(receiptSummary({ route: 'fast', capabilities: [{ name: 'search' }, { version: '2' }, null], tools: [{ name: 'read' }] }))
      .toEqual({ route: 'fast', model: null, time: null })
  })

  it('states the route-to-model relation in words for screen readers', () => {
    expect(receiptLabel({
      route: 'analysis/high', model: 'glm-5.2', cost: '$0.0041', time: '3.8s',
      capabilities: [{ name: 'search', version: '2' }],
    })).toBe('Routed via analysis/high to model glm 5.2, 4s')
  })

  it('labels partial receipts without naming a field the receipt lacks', () => {
    expect(receiptLabel({ route: 'analysis/high', time: '3.8s' })).toBe('Routed via analysis/high, 4s')
    expect(receiptLabel({ model: 'glm-5.2' })).toBe('Model glm 5.2')
    expect(receiptLabel({ cost: '$0.0041' })).toBe('')
    expect(receiptLabel({})).toBe('')
    expect(receiptLabel(null)).toBe('')
  })

  it('projects every field the line does not show, in record order, and nothing twice', () => {
    expect(receiptRows({
      route: 'fast', model: 'glm-5.2', cost: '$0.04', time: '1.8s',
      capabilities: [{ name: 'search', version: '2' }, { name: 'files', version: '1' }],
    })).toEqual([
      { label: 'Cost', value: '$0.04', route: false },
      { label: 'Capability', value: 'search@2', route: false },
      { label: 'Capability', value: 'files@1', route: false },
    ])
    expect(receiptRows({ route: 'fast', model: 'glm-5.2', time: '1.8s' })).toEqual([])
    expect(receiptRows({ time: '0.4s' })).toEqual([])
    expect(receiptRows(null)).toEqual([])
  })

  it('projects the local run record as Tokens, Turns and Tools rows after Cost', () => {
    expect(receiptRows({
      model: 'ollama/llama3.2:3b', cost: '$0.013 est.', time: '4.2s',
      tokens: { input: 11414, output: 64, cacheRead: 1200, cacheWrite: 0, reasoning: 0, total: 12678 },
      turns: 2,
      tools: [{ name: 'bash', calls: 1, failed: 1 }, { name: 'grep', calls: 4, failed: 0 }],
    })).toEqual([
      { label: 'Cost', value: '$0.013 est.', route: false },
      { label: 'Tokens', value: '11,414 in, 64 out, 1,200 cached', route: false },
      { label: 'Turns', value: '2', route: false },
      { label: 'Tools', value: 'bash 1 (1 failed), grep 4', route: false },
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
      { label: 'Cost', value: '$0.04', route: false },
      { label: 'Memory', value: 'lease, 2 files', files: ['/Documents/Muniment/lease.pdf', '/Documents/Muniment/notes.md'], route: false },
      { label: 'Memory', value: 'missing clause, 0 files', files: [], route: false },
      { label: 'Capability', value: 'files@1', route: false },
    ])
  })
  it('omits absent receipt fields and never repeats the line', () => {
    expect(receiptRows({ model: 'glm-5.2', time: '1.8s' })).toEqual([])
    expect(receiptRows({ model: 'glm-5.2', time: '1.8s', turns: 1 })).toEqual([{ label: 'Turns', value: '1', route: false }])
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

  it('preserves Pi acquisition when history restores an active run', () => {
    const messages = historyMessages([{ runId: 'r', phase: 'acquiring-pi', text: '' }])
    expect(unsettledRun(messages).phase).toBe('acquiring-pi')
    expect(runAnnouncement(messages[0].run)).toBe('Reply setup has started. Please wait.')
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

it('keeps a saved partial reply and calls a generic start failure an interruption', () => {
  const run = { phase: 'failed', text: 'The first part of the reply.', failureReason: 'The reply could not be started.' }
  expect(runFailureMessage(run)).toBe('The reply stopped before it finished.')
  expect(run.text).toBe('The first part of the reply.')
})

it('compares model and classifier usage in separate columns', () => {
  const receipt = { model: 'openai/gpt-5.6-luna', cost: '$0.002 est.', tokens: { input: 11736, output: 5 }, turns: 1, classifiers: [{ model: 'typesafe/jev-latest', cost: 0.000040, tokens: { input: 942, output: 151 } }] }
  expect(receiptUsageColumns(receipt)).toEqual([
    { model: 'openai/gpt 5.6 luna', cost: '$0.002 est.', tokens: '11,736 in, 5 out' },
    { model: 'typesafe/jev latest', cost: '$0.000040 est.', tokens: '942 in, 151 out' },
  ])
  expect(receiptRows(receipt)).toEqual([{ label: 'Turns', value: '1', route: false }])
  expect(receiptUsageColumns({ classifiers: [{ model: 'private', cost: null, tokens: null }] })[1]).toEqual({ model: 'private', cost: 'Unavailable', tokens: 'Unavailable' })
  expect(receiptUsageColumns(null)).toEqual([])
})

it('shows restored routing evidence without treating model confidence as answer quality', () => {
  const receipt = JSON.parse(JSON.stringify({ routing: [{ account: 'Work', selected_model: 'openai/model', decision: 'Classifier selected the model', confidence: 0.8, classification_ms: 42, exclusions: ['anthropic/model: Account is turned off.'], fallback_causes: ['Personal answered 429.'] }] }))
  const rows = receiptRows(receipt)
  expect(rows).toContainEqual({ label: 'Account', value: 'Work', route: false })
  expect(rows).toContainEqual({ label: 'Routing confidence', value: '80% · Model selection, not answer quality', route: false })
  expect(rows).toContainEqual({ label: 'Classification time', value: '42 ms', route: false })
  expect(rows).toContainEqual({ label: 'Excluded model', value: 'anthropic/model: Account is turned off.', route: false })
  expect(rows).toContainEqual({ label: 'Fallback cause', value: 'Personal answered 429.', route: false })
  expect(receiptRows({})).toEqual([])
})

it('uses runtime routing stages and clears them when reply text arrives', () => {
  let previous = { phase: 'thinking', text: '', stage: 'thinking', turnStarted: true }
  for (const [routingStage, label] of [['choosing-model', 'Choosing model'], ['waiting-for-account', 'Waiting for account'], ['fallback', 'Trying another route']]) {
    const next = { ...previous, routingStage }
    expect(stageWord(runStage(previous, next))).toBe(label)
    previous = { ...next, stage: runStage(previous, next) }
  }
  expect(runStage(previous, { ...previous, routingStage: undefined, text: 'Answer' })).toBe('writing')
  expect(runStage(previous, { ...previous, routingStage: undefined })).toBe('thinking')
})

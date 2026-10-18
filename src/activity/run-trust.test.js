import { describe, expect, it } from 'vitest'
import { compactTokens, estimateCost, irreversibleLines, rewindFiles, rewindOutcome, rewindUnavailable, runLineLabel, runLineParts, runTimeline, turnTokens } from './run-trust.js'

const modes = [{ id: 'bypass', title: 'Bypass permissions' }, { id: 'manual', title: 'Ask before changes' }]

describe('the run line', () => {
  it('names the machine, model, mode, time, tokens and the API estimate', () => {
    const run = { receipt: { model: 'anthropic/claude-sonnet-4.5', time: '42.0s', tokens: { input: 15200, output: 3040, total: 18240 }, cost_usd: 0.0912, permission_mode: 'bypass' } }
    const parts = runLineParts(run, { machine: 'Studio', modes })
    expect(parts).toEqual({ machine: 'Studio', model: 'Anthropic · Claude Sonnet 4.5', mode: 'Bypass permissions', time: '42s', tokens: '18k tokens', cost: '$0.09' })
    expect(runLineLabel(parts)).toBe('On Studio, model Anthropic · Claude Sonnet 4.5, mode Bypass permissions, took 42s, 18k tokens, $0.09 API estimate')
    // The newest turn names where its model and mode come from.
    const sourced = runLineParts(run, { machine: 'Studio', modes, sources: { model: 'from project Homelab', mode: 'default' } })
    expect([sourced.model, sourced.mode]).toEqual(['Anthropic · Claude Sonnet 4.5 (from project Homelab)', 'Bypass permissions (default)'])
  })

  it('leaves out what the run did not record, and a live run shows its elapsed time and the mode in force', () => {
    expect(runLineParts({ receipt: null }, { machine: 'This computer', elapsed: '3s', liveMode: 'Ask before changes' }))
      .toEqual({ machine: 'This computer', model: null, mode: 'Ask before changes', time: '3s', tokens: null, cost: null })
  })

  it('formats tokens and costs compactly and never shows a partial sum as a bill', () => {
    expect([compactTokens(812), compactTokens(1250), compactTokens(18240), compactTokens(2_400_000)]).toEqual(['812', '1.3k', '18k', '2.4M'])
    expect(turnTokens({ input: 10, output: 5 })).toBe(15)
    expect(turnTokens({ input: 0, output: 0 })).toBe(null)
    expect([estimateCost(0.0912), estimateCost(0.0042), estimateCost(0.00001), estimateCost(0), estimateCost(undefined)]).toEqual(['$0.09', '$0.004', '<$0.001', '$0.00', null])
  })

  it('lists every tool call in order with its state and duration', () => {
    const test = { kind: 'command', icon: 'square-terminal', headline: 'Run `npm test`', progress: 'Running npm test', result: 'Exit 0 in 26 s' }
    const edit = { kind: 'edit', icon: 'square-pen', headline: 'Edit a.js', progress: 'Editing a.js', result: 'Failed: no match', path: 'src/a.js' }
    const timeline = runTimeline([
      { effectId: 'a', displayName: 'bash', status: 'completed', input: '{"command":"npm test"}', startedAt: '2026-10-18T12:00:00Z', finishedAt: '2026-10-18T12:00:26Z', presentation: test },
      { effectId: 'b', displayName: 'edit', status: 'failed', input: '{"path":"src/a.js"}', presentation: edit },
    ])
    expect(timeline).toEqual([
      { id: 'a', presentation: test, icon: 'square-terminal', label: 'Run `npm test`', result: 'Exit 0 in 26 s', failed: false, running: false, state: 'Completed', duration: '26s' },
      { id: 'b', presentation: edit, icon: 'square-pen', label: 'Edit a.js', result: 'Failed: no match', failed: true, running: false, state: 'Failed', duration: '' },
    ])
  })
})

describe('a rewind', () => {
  it('names each kind of effect it cannot take back', () => {
    expect(irreversibleLines([{ kind: 'command', count: 2, labels: ['git push'] }, { kind: 'network', count: 1, labels: [] }, { kind: 'unknown', count: 1 }])).toEqual([
      { kind: 'command', headline: '2 commands ran.', note: 'Their changes inside the folder are restored. Anything else they did stays.', labels: ['git push'] },
      { kind: 'network', headline: '1 web request went out.', note: 'What they sent stays sent.', labels: [] },
    ])
  })

  it('shows what files return, go and stay, or why files cannot return', () => {
    expect(rewindFiles({ files_available: true, restore: ['a.md'], remove: ['b.md'], cannot_restore: [{ path: 'c.mov', reason: 'too_large' }], not_covered: ['.git'] }))
      .toEqual({ available: true, note: '', restore: ['a.md'], remove: ['b.md'], cannot: [{ path: 'c.mov', reason: 'too large to save' }], notCovered: ['.git'] })
    expect(rewindFiles({ files_available: false, unavailable: 'The folder holds more than 20000 files, so its files were not saved.', not_covered: [] }).note).toContain('20000 files')
    expect(rewindFiles({ files_available: true, restore: [], remove: [], cannot_restore: [], not_covered: [] }).note).toBe('No file changed since this message.')
  })

  it('never reports a partial restore as done', () => {
    const partial = rewindOutcome({ status: 'partial', turns: 2, conversation: 'restored', files: { restored: ['a.md'], removed: [], failed: [{ path: 'b.md', reason: 'Permission denied.' }], cannot_restore: [], not_covered: ['.git'], complete: false } })
    expect(partial).toEqual({ done: false, headline: 'Partly rewound 2 turns, restored 1 file.', details: ['b.md: Permission denied.'], notCovered: ['.git'] })
    const failedConversation = rewindOutcome({ status: 'partial', turns: 1, conversation: { failed: 'The saved copy is missing.' }, files: null })
    expect(failedConversation.done).toBe(false)
    expect(failedConversation.details).toEqual(['The saved conversation was not restored: The saved copy is missing.', 'Files were left as they are.'])
    expect(rewindOutcome({ status: 'rewound', turns: 1, conversation: 'restored', files: { restored: ['a.md'], removed: ['b.md'], failed: [], cannot_restore: [], not_covered: [], complete: true } }))
      .toEqual({ done: true, headline: 'Rewound 1 turn, restored 1 file and removed 1 file.', details: [], notCovered: [] })
  })

  it('is not offered on a group, another machine\'s thread or during a reply', () => {
    expect(rewindUnavailable({})).toBe(null)
    expect(rewindUnavailable({ replica: true, owner: { name: 'Studio' } })).toBe('Studio runs this thread. Rewind it there.')
    expect(rewindUnavailable({ group: true })).toBe('Group threads cannot rewind.')
    expect(rewindUnavailable({ running: true })).toBe('Stop the reply to rewind.')
  })
})

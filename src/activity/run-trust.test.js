import { describe, expect, it } from 'vitest'
import { irreversibleLines, rewindFiles, rewindOutcome, rewindUnavailable, runContextRows, turnSources } from './run-trust.js'

const modes = [{ id: 'bypass', title: 'Bypass permissions' }, { id: 'manual', title: 'Ask before changes' }]

describe('the receipt rows for where a turn ran', () => {
  it('names the machine and the mode, and where the newest turn takes its model and mode from', () => {
    const run = { receipt: { model: 'anthropic/claude-sonnet-4.5', permission_mode: 'bypass' } }
    expect(runContextRows(run, { machine: 'Studio', modes })).toEqual([
      { label: 'Machine', value: 'Studio', route: false },
      { label: 'Mode', value: 'Bypass permissions', route: false },
    ])
    expect(runContextRows(run, { machine: 'Studio', modes, sources: { model: 'from project Homelab', mode: 'this thread' } }).map((row) => [row.label, row.value])).toEqual([
      ['Machine', 'Studio'], ['Mode', 'Bypass permissions (this thread)'], ['Model source', 'from project Homelab'],
    ])
  })

  it('keeps an earlier turn\'s sources after a later turn arrives', () => {
    const sources = turnSources()
    const first = { id: 'run-1' }, second = { id: 'run-2' }
    const now = { model: 'from project Homelab', mode: 'this thread' }
    expect(sources(first, 'run-1', () => now)).toEqual(now)
    expect(sources(second, 'run-2', () => ({ model: null, mode: 'default' }))).toEqual({ model: null, mode: 'default' })
    expect(sources(first, 'run-2', () => ({}))).toEqual(now)
    expect(sources({ id: 'run-0' }, 'run-2', () => now)).toEqual({})
  })

  it('leaves out what the run did not record', () => {
    expect(runContextRows({ receipt: { permission_mode: 'unknown' } }, { modes })).toEqual([])
    expect(runContextRows({ receipt: null }, { machine: 'This computer' }).map((row) => row.label)).toEqual(['Machine'])
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

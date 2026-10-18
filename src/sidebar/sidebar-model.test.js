import { describe, expect, it } from 'vitest'
import { attention, holdRows, markRead, readMarks, runStates, shownRows, sidebarInput, statusTone } from './sidebar-model.js'

function memoryStorage(values = {}) {
  return { getItem: (key) => values[key] ?? null, setItem: (key, value) => { values[key] = value }, values }
}

describe('sidebar model', () => {
  it('sets the read baseline once and keeps read times on the device', () => {
    const storage = memoryStorage()
    const first = readMarks(storage, new Date('2026-10-18T10:00:00Z'))
    expect(first).toEqual({ read: {}, baseline: '2026-10-18T10:00:00.000Z' })
    const marked = markRead(first, 'thread-1', new Date('2026-10-18T11:00:00Z'), storage)
    expect(readMarks(storage, new Date('2026-10-19T00:00:00Z'))).toEqual({ read: { 'thread-1': '2026-10-18T11:00:00.000Z' }, baseline: '2026-10-18T10:00:00.000Z' })
    expect(marked.read['thread-1']).toBe('2026-10-18T11:00:00.000Z')
    expect(readMarks({ getItem: () => '{', setItem: () => {} }).read).toEqual({})
  })

  it('reads working and failed threads from the newest run of each', () => {
    const { working, failed } = runStates([
      { thread_id: 'a', status: 'running' },
      { thread_id: 'b', status: 'failed' },
      { thread_id: 'b', status: 'running' },
      { thread_id: 'c', status: 'completed' },
    ])
    expect([...working]).toEqual(['a'])
    expect([...failed]).toEqual(['b'])
  })

  it('points each needs-you item at its thread or agent and leaves out offline machines', () => {
    expect(attention([
      { kind: 'input', run: { thread_id: 't1' }, agent_id: 'a1' },
      { kind: 'approval', approval: { agent_id: 'a2' } },
      { kind: 'offline', id: 'box' },
    ])).toEqual([
      { kind: 'input', thread_id: 't1', agent_id: 'a1' },
      { kind: 'approval', thread_id: null, agent_id: 'a2' },
    ])
  })

  it('hands core every thread, project, agent and choice the window holds', () => {
    const input = sidebarInput({
      summaries: [{ threadId: 't1', title: 'Notes', updatedAt: '2026-10-18T10:00:00Z', runtimeId: 'box' }],
      agentListing: { primaryThreads: { a1: 'home' }, threads: { a1: [{ thread_id: 'home', title: 'Runs', updated_at: '2026-10-18T09:00:00Z' }] }, latestRuns: {} },
      agents: [{ id: 'a1', name: 'Watch', projectId: 'p1', routines: [{ enabled: true, trigger: { kind: 'webhook' } }] }],
      groups: [{ thread_id: 'g1', name: 'Launch room' }],
      catalog: { projects: { p1: 'Homelab' }, threads: { t1: 'p1' } },
      organization: { t1: { pinned: true, archived: false } },
      held: new Map([['t1', 'recent']]),
      machineOf: (id) => id === 'box' ? 'Build box' : null,
    })
    expect(input.threads).toEqual([
      { id: 't1', title: 'Notes', updated_at: '2026-10-18T10:00:00Z', machine: 'Build box' },
      { id: 'home', title: 'Runs', updated_at: '2026-10-18T09:00:00Z', machine: null },
      { id: 'g1', title: 'Launch room', updated_at: '', machine: null },
    ])
    expect(input.agents).toEqual([{ id: 'a1', name: 'Watch', project_id: 'p1', home_thread_id: 'home', paused_reason: null, routines: [{ enabled: true, proposed: false, trigger: { kind: 'webhook' } }] }])
    expect(input.projects).toEqual([{ id: 'p1', name: 'Homelab', owner: null }])
    expect(input.agent_threads).toEqual({ home: 'a1' })
    expect(input.pinned).toEqual(['t1'])
    expect(input.held).toEqual({ t1: 'recent' })
  })

  it('holds rows where the answer placed them and orders the shortcuts once each', () => {
    const row = (id) => ({ thread_id: id })
    const sidebar = { needs_you: [row('n')], working: [row('w')], pinned: [row('p')], projects: [{ id: 'p1', threads: [row('r')] }], recent: [row('r')], settled: [row('s')], archived: [], results: [] }
    expect([...holdRows(new Map(), sidebar)]).toEqual([['r', 'recent'], ['s', 'settled']])
    expect(shownRows(sidebar, { expanded: new Set(['p1']), fresh: true }).map((entry) => entry.thread_id)).toEqual(['n', 'w', 'p', 'r', null])
    expect(shownRows(sidebar, { workingOpen: false, settledOpen: true }).map((entry) => entry.thread_id)).toEqual(['n', 'p', 'r', 's'])
    expect(statusTone('needs_you')).toBe('warning')
    expect(statusTone('read')).toBe('neutral')
  })
})

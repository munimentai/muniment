import { describe, expect, it } from 'vitest'
import { agentColor } from '../lib/agent-avatar.js'
import { attention, freshIndex, listedRows, markRead, readMarks, runStates, shownRows, sidebarInput, statusTone } from './sidebar-model.js'

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
    const { working, failed, since } = runStates([
      { thread_id: 'a', status: 'running', created_at: '2026-10-18T10:00:00Z' },
      { thread_id: 'b', status: 'failed' },
      { thread_id: 'b', status: 'running' },
      { thread_id: 'c', status: 'completed' },
    ])
    expect([...working]).toEqual(['a'])
    expect([...failed]).toEqual(['b'])
    expect([...since]).toEqual([['a', '2026-10-18T10:00:00Z']])
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
      agentChats: [{ threadId: 'c1', title: 'Inbox brief', updatedAt: '2026-10-18T11:00:00Z', agentId: 'a1' }, { threadId: 't1', title: '', updatedAt: '2026-10-18T12:00:00Z', agentId: null }],
      catalog: { projects: { p1: 'Homelab' }, threads: { t1: 'p1' } },
      organization: { t1: { pinned: true, archived: false } },
      machineOf: (id) => id === 'box' ? 'Build box' : null,
    })
    expect(input.threads).toEqual([
      { id: 't1', title: 'Notes', updated_at: '2026-10-18T12:00:00Z', machine: 'Build box' },
      { id: 'home', title: 'Runs', updated_at: '2026-10-18T09:00:00Z', machine: null },
      { id: 'g1', title: 'Launch room', updated_at: '', machine: null },
      { id: 'c1', title: 'Inbox brief', updated_at: '2026-10-18T11:00:00Z', machine: null },
    ])
    expect(input.agents).toEqual([{ id: 'a1', name: 'Watch', project_id: 'p1', home_thread_id: 'home', paused_reason: null, routines: [{ enabled: true, proposed: false, trigger: { kind: 'webhook' } }], color: agentColor({ id: 'a1', name: 'Watch' }) }])
    expect(input.projects).toEqual([{ id: 'p1', name: 'Homelab', owner: null }])
    // A saved agent's creation chat sits with its agent.
    expect(input.agent_threads).toEqual({ home: 'a1', c1: 'a1' })
    expect(input.pinned).toEqual(['t1'])
    expect(input).not.toHaveProperty('held')
    expect(input).not.toHaveProperty('filter')
  })

  it('orders the shortcuts by the Projects list once each, a new thread after the pinned rows', () => {
    const row = (id) => ({ thread_id: id })
    const sidebar = { projects: [
      { id: 'pin', kind: 'thread', pinned: true, threads: [row('pin')] },
      { id: 'p1', kind: 'project', pinned: false, threads: [row('a'), row('b')] },
      { id: 'solo', kind: 'thread', pinned: false, threads: [row('solo')] },
    ], archived: [row('gone')], results: [] }
    expect(freshIndex(sidebar)).toBe(1)
    expect(shownRows(sidebar, { expanded: new Set(['p1']), fresh: true }).map((entry) => entry.thread_id)).toEqual(['pin', null, 'a', 'b', 'solo'])
    expect(shownRows(sidebar).map((entry) => entry.thread_id)).toEqual(['pin', 'solo'])
    expect(listedRows(sidebar).map((entry) => entry.thread_id)).toEqual(['pin', 'a', 'b', 'solo'])
    expect(statusTone('needs_you')).toBe('warning')
    expect(statusTone('read')).toBe('neutral')
  })
})

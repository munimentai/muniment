import { describe, expect, it, vi } from 'vitest'
import { agentMachine, agentRequest, agentRuntimes, agentThreads, applyRunUpdate, listAgents, listRuns } from './agent-runtime.js'

const studio = { id: '018f0000-0000-7000-8000-000000000042', name: 'Studio', kind: 'desktop', status: 'online', local: false }
const box = { id: '018f0000-0000-7000-8000-000000000043', name: 'Build box', kind: 'container', status: 'offline', local: false }
const list = { selected: 'local', runtimes: [{ id: 'local', local: true, kind: 'local', name: 'This computer', status: 'online' }, studio, box] }
const run = (id, runtime, updated, extra = {}) => ({ id, agent_id: 'a', thread_id: `t-${id}`, status: 'completed', trigger: { kind: 'cron' }, created_at: updated, updated_at: updated, ...extra, runtime })

describe('agents across machines', () => {
  it('sends each request to the runtime it names', async () => {
    const tauri = { invoke: vi.fn(async () => ({})) }
    await agentRequest(tauri, studio.id, 'agent.read', { id: 'a' })
    await agentRequest(tauri, null, 'agent.list')
    expect(tauri.invoke.mock.calls).toEqual([
      ['agent_request', { runtime: studio.id, operation: 'agent.read', body: { id: 'a' } }],
      ['agent_request', { runtime: 'local', operation: 'agent.list', body: {} }],
    ])
  })

  it('lists this computer and every paired machine that answers, each agent with its machine', async () => {
    expect(agentRuntimes(list).map((runtime) => runtime.id)).toEqual(['local', studio.id])
    expect(agentRuntimes(null).map((runtime) => runtime.id)).toEqual(['local'])
    const tauri = { invoke: vi.fn(async (_, { runtime }) => runtime === 'local'
      ? { agents: [{ id: 'a1', name: 'Scout' }], primary_threads: { a1: 't1' }, latest_runs: { a1: run('r1', 'local', '2026-01-01T00:00:00Z', { agent_id: 'a1', thread_id: 't9' }) }, errors: [{ folder: 'Broken', error: 'The agent settings are invalid.' }] }
      : { agents: [{ id: 'a2', name: 'Editor' }], primary_threads: {}, latest_runs: {}, errors: [] }) }
    const listing = await listAgents(tauri, list)
    expect(listing.agents).toEqual([{ id: 'a1', name: 'Scout', runtimeId: 'local' }, { id: 'a2', name: 'Editor', runtimeId: studio.id }])
    expect(listing.errors).toEqual([{ folder: 'Broken', error: 'The agent settings are invalid.', runtimeId: 'local' }])
    expect(listing.latestRuns.a1.runtimeId).toBe('local')
    expect(agentThreads(listing)).toEqual({ t1: 'a1', t9: 'a1' })
  })

  it('names every thread of each agent from its own machine', async () => {
    const tauri = { invoke: vi.fn(async (_, { runtime, operation, body }) => {
      if (operation === 'agent.threads') {
        if (runtime !== 'local') throw new Error('The agent action is unknown.')
        return { threads: [{ thread_id: 't1', title: 'Morning', updated_at: '2026-01-01T09:00:00Z', primary: true }, { thread_id: 't2', title: 'Older', updated_at: '2025-12-30T09:00:00Z', primary: false }] }
      }
      return runtime === 'local' ? { agents: [{ id: 'a1' }], primary_threads: { a1: 't1' } } : { agents: [{ id: 'a2' }], primary_threads: { a2: 't5' } }
    }) }
    const listing = await listAgents(tauri, list)
    expect(tauri.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'agent.threads', body: { id: 'a1' } })
    expect(listing.threads).toEqual({ a1: [expect.objectContaining({ thread_id: 't1' }), expect.objectContaining({ thread_id: 't2' })] })
    expect(agentThreads(listing)).toEqual({ t1: 'a1', t2: 'a1', t5: 'a2' })
  })

  it('leaves out a paired machine that fails and fails only when nothing answers', async () => {
    const failing = (failed) => ({ invoke: vi.fn(async (_, { runtime, operation }) => { if (failed.includes(runtime)) throw new Error(`${runtime} down`); return operation === 'agent.threads' ? { threads: [] } : { agents: [{ id: runtime }] } }) })
    const listing = await listAgents(failing([studio.id]), list)
    expect(listing.agents.map((agent) => agent.id)).toEqual(['local'])
    expect(listing.unreachable).toEqual([studio.id])
    expect((await listAgents(failing(['local']), list)).agents.map((agent) => agent.id)).toEqual([studio.id])
    await expect(listAgents(failing(['local', studio.id]), list)).rejects.toThrow('local down')
  })

  it('merges run pages newest first and applies each run update once', async () => {
    const tauri = { invoke: vi.fn(async (_, { runtime, body }) => ({ runs: runtime === 'local' ? [run('r1', 'local', '2026-01-01T10:00:00Z')] : [run('r2', 's', '2026-01-01T11:00:00Z')], body })) }
    const runs = await listRuns(tauri, list)
    expect(runs.map((entry) => [entry.id, entry.runtimeId])).toEqual([['r2', studio.id], ['r1', 'local']])
    expect(tauri.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'agent.runs', body: { limit: 50 } })
    await listRuns(tauri, list, { agentId: 'a', runtimeId: studio.id, limit: 20 })
    expect(tauri.invoke).toHaveBeenLastCalledWith('agent_request', { runtime: studio.id, operation: 'agent.runs', body: { agent_id: 'a', limit: 20 } })

    const updated = applyRunUpdate(runs, { runtimeId: 'local', run: { ...run('r1', 'local', '2026-01-01T12:00:00Z'), status: 'failed' } })
    expect(updated.map((entry) => [entry.id, entry.status])).toEqual([['r1', 'failed'], ['r2', 'completed']])
    expect(applyRunUpdate(updated, { runtimeId: studio.id, run: run('r3', 's', '2026-01-01T13:00:00Z') }, 2).map((entry) => entry.id)).toEqual(['r3', 'r1'])
    expect(applyRunUpdate(updated, { runtimeId: 'local' })).toBe(updated)
  })

  it('names the machine an agent lives on', () => {
    expect(agentMachine(list, { runtimeId: studio.id }).name).toBe('Studio')
    expect(agentMachine(list, {}).kind).toBe('local')
    expect(agentMachine(null, { runtimeId: 'gone' }).name).toBe('Paired machine')
  })
})

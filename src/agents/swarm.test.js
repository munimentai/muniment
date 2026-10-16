import { describe, expect, it, vi } from 'vitest'
import { applyApproval, directLinks, emptySwarm, linkMachines, listSwarm, memberMachine, mergeSwarm, moveOutcome, moveRequest, postTarget, swarmRoster, swarmThreads, windowRuntime } from './swarm.js'

const LAPTOP = 'core-laptop'
const BOX = 'core-box'
const STUDIO = 'core-studio'

const member = (runtime_id, runtime_name, form_factor, extra = {}) => ({ runtime_id, runtime_name, form_factor, online: true, heartbeat_at: '2026-01-01T14:00:00Z', last_client_at: null, links: [], self: false, link: null, ...extra })

function answers() {
  return [
    {
      runtimeId: 'local',
      members: { self: LAPTOP, absence: { paused: false, paused_at: null, last_client_at: '2026-01-01T13:00:00Z' }, members: [member(LAPTOP, 'Laptop', 'laptop', { self: true }), member(BOX, 'Build box', 'server', { link: { direction: 'outbound', state: 'connected' } }), member(STUDIO, 'Studio', 'desktop', { online: false })] },
      agents: { agents: [
        { agent_id: 'a1', name: 'Scout', primary_runtime: LAPTOP, epoch: 1, updated_at: '2026-01-01T10:00:00Z', primary_thread: 't1' },
        { agent_id: 'a3', name: 'Editor', primary_runtime: STUDIO, epoch: 0, updated_at: '2026-01-01T10:00:00Z', primary_thread: 't3' },
        { agent_id: 'gone', name: 'Old', primary_runtime: LAPTOP, epoch: 0, updated_at: '2026-01-01T10:00:00Z', deleted: true },
      ] },
      groups: { groups: [{ group_id: 'g1', name: 'Launch', owner_runtime: BOX, thread_id: 'gt1', members: ['a1', 'a3'], updated_at: '2026-01-01T10:00:00Z' }] },
      approvals: { approvals: [{ approval_id: 'r1:g1', runtime_id: BOX, state: 'pending', created_at: '2026-01-01T14:01:00Z', request: { kind: 'confirm' } }] },
    },
    {
      runtimeId: 'box',
      members: { self: BOX, absence: { paused: true, paused_at: '2026-01-01T12:00:00Z', last_client_at: '2025-12-01T00:00:00Z' }, members: [member(BOX, 'Build box', 'server', { self: true }), member(LAPTOP, 'Laptop', 'laptop')] },
      agents: { agents: [{ agent_id: 'a1', name: 'Scout moved', primary_runtime: BOX, epoch: 2, updated_at: '2026-01-01T09:00:00Z', primary_thread: 't1' }] },
      groups: null,
      approvals: { approvals: [{ approval_id: 'r1:g1', runtime_id: BOX, state: 'pending', created_at: '2026-01-01T14:01:00Z' }] },
    },
  ]
}

describe('the merged swarm', () => {
  it('names each machine the app reaches by its core id and keeps one entry per member', () => {
    const swarm = mergeSwarm(answers())
    expect(swarm.self).toEqual({ local: LAPTOP, box: BOX })
    expect(swarm.members.map((entry) => entry.runtime_id)).toEqual([LAPTOP, BOX, STUDIO])
    expect(swarm.members.every((entry) => !('self' in entry) && !('link' in entry))).toBe(true)
    expect(windowRuntime(swarm, BOX)).toBe('box')
    expect(windowRuntime(swarm, STUDIO)).toBeNull()
  })

  it('takes the highest epoch, drops deleted agents and pauses when any member paused', () => {
    const swarm = mergeSwarm(answers())
    expect(swarm.agents.map((entry) => [entry.agent_id, entry.name, entry.primary_runtime])).toEqual([['a1', 'Scout moved', BOX], ['a3', 'Editor', STUDIO]])
    expect(swarm.absence.paused).toBe(true)
  })

  it('sends a group or an approval to the machine that owns it when the app reaches it', () => {
    const swarm = mergeSwarm(answers())
    expect(swarm.groups[0].runtimeId).toBe('box')
    expect(swarm.approvals).toHaveLength(1)
    expect(swarm.approvals[0].runtimeId).toBe('box')
  })

  it('asks each machine for its members, agents, groups and approvals and survives a refusal', async () => {
    const invoke = vi.fn(async (_command, { runtime, operation }) => {
      if (runtime === 'box') throw new Error('refused')
      if (operation === 'swarm.members') return answers()[0].members
      return { agents: [], groups: [], approvals: [] }
    })
    const swarm = await listSwarm({ invoke }, { runtimes: [{ id: 'local', local: true }, { id: 'box', status: 'online' }, { id: 'off', status: 'offline' }] })
    expect(invoke.mock.calls.map(([, payload]) => `${payload.runtime} ${payload.operation}`).sort()).toEqual([
      'box agent.approvals', 'box group.list', 'box swarm.agents', 'box swarm.members',
      'local agent.approvals', 'local group.list', 'local swarm.agents', 'local swarm.members',
    ])
    expect(swarm.self).toEqual({ local: LAPTOP })
  })
})

describe('the roster', () => {
  it('lists the agents machines answered for, then the agents only the directory names, as replicas', () => {
    const swarm = mergeSwarm(answers())
    const roster = swarmRoster({ agents: [{ id: 'a1', name: 'Scout', runtimeId: 'box' }] }, swarm)
    expect(roster.map((agent) => [agent.id, agent.replica, agent.primaryRuntime, agent.online])).toEqual([['a1', false, BOX, true], ['a3', true, STUDIO, false]])
    expect(roster[1].primaryMachine).toMatchObject({ name: 'Studio', formFactor: 'desktop', online: false })
    expect(roster[1].primaryThread).toBe('t3')
    expect(swarmThreads(swarm)).toEqual({ t1: 'a1', t3: 'a3' })
  })

  it('names this computer and falls back to the runtime list for a machine with no swarm', () => {
    const swarm = mergeSwarm(answers())
    expect(memberMachine(swarm, LAPTOP)).toMatchObject({ name: 'This computer', kind: 'local', online: true })
    expect(memberMachine(emptySwarm(), 'x')).toMatchObject({ name: 'Unknown machine', online: false })
    const roster = swarmRoster({ agents: [{ id: 'a', name: 'A', runtimeId: 'local' }] }, emptySwarm())
    expect(roster[0]).toMatchObject({ primaryRuntime: null, primaryMachine: null, online: true, replica: false })
  })
})

describe('posting', () => {
  const swarm = mergeSwarm(answers())
  it('posts to a group thread and to a replica of another member\'s agent thread', () => {
    expect(postTarget({ threadId: 'gt1', threadRuntime: 'local', groups: swarm.groups, swarm })).toMatchObject({ runtime: 'local', body: { thread_id: 'gt1' }, group: { group_id: 'g1' } })
    expect(postTarget({ threadId: 't3', threadRuntime: 'local', groups: [], agent: { primaryRuntime: STUDIO }, swarm })).toEqual({ runtime: 'local', body: { thread_id: 't3' }, group: null })
  })
  it('runs a message in a thread on the agent\'s primary machine or in an ordinary thread', () => {
    expect(postTarget({ threadId: 't1', threadRuntime: 'box', groups: [], agent: { primaryRuntime: BOX }, swarm })).toBeNull()
    expect(postTarget({ threadId: 'x', threadRuntime: 'local', groups: [], agent: null, swarm })).toBeNull()
    expect(postTarget({ threadId: null, swarm })).toBeNull()
  })
})

describe('linking', () => {
  it('runs the invitation on one machine and redeems it on the other', async () => {
    const invoke = vi.fn(async (_command, { operation }) => operation === 'swarm.invite' ? { link: 'muniment://pair?v=1&id=x' } : { member: { runtime_id: BOX } })
    expect(await linkMachines({ invoke }, 'local', 'box')).toEqual({ runtime_id: BOX })
    expect(invoke.mock.calls.map(([, payload]) => [payload.runtime, payload.operation, payload.body])).toEqual([
      ['local', 'swarm.invite', {}],
      ['box', 'swarm.join', { link: 'muniment://pair?v=1&id=x' }],
    ])
    await expect(linkMachines({ invoke }, 'box', 'box')).rejects.toThrow('Choose two different machines.')
  })
  it('lists a machine\'s direct links from its own view', () => {
    expect(directLinks(answers()[0].members).map((entry) => entry.runtime_id)).toEqual([BOX])
  })
})

describe('moving an agent', () => {
  const swarm = mergeSwarm(answers())
  const scout = { id: 'a1', runtimeId: 'box', primaryRuntime: BOX }
  it('sends a move through the machine that holds the agent', () => {
    expect(moveRequest({ agent: scout, to: LAPTOP, swarm })).toEqual({ runtime: 'box', body: { id: 'a1', to: LAPTOP, force: false }, primaryOnline: true })
    expect(moveRequest({ agent: scout, to: BOX, swarm }).error).toBe('Choose another machine.')
  })
  it('asks for a forced move when the primary does not answer and sends it to the target', () => {
    const editor = { id: 'a3', runtimeId: 'local', primaryRuntime: STUDIO }
    expect(moveRequest({ agent: editor, to: LAPTOP, swarm })).toMatchObject({ primaryOnline: false, canForce: true })
    expect(moveRequest({ agent: editor, to: LAPTOP, swarm, force: true })).toEqual({ runtime: 'local', body: { id: 'a3', to: LAPTOP, force: true }, primaryOnline: false, forced: true })
  })
  it('refuses a force while the primary answers or when the app does not reach the target', () => {
    expect(moveRequest({ agent: scout, to: LAPTOP, swarm, force: true }).error).toBe('The primary machine answers. Move without forcing.')
    const editor = { id: 'a3', runtimeId: 'local', primaryRuntime: STUDIO }
    const unreached = { ...swarm, self: { box: BOX } }
    expect(moveRequest({ agent: editor, to: LAPTOP, swarm: unreached, force: true }).error).toBe('Pair this device with the target machine to force the move.')
  })
  it('names each move answer', () => {
    expect(moveOutcome('moved', 'Build box')).toBe('Moved to Build box.')
    expect(moveOutcome('requested', 'Build box')).toMatch(/^Move to Build box requested\./)
    expect(moveOutcome('forced', 'Laptop')).toMatch(/^Forced the move to Laptop\./)
  })
})

describe('approval events', () => {
  it('adds a pending approval once and drops a resolved one', () => {
    const pending = { approval_id: 'x', state: 'pending', created_at: '2026-01-01T14:00:00Z' }
    let approvals = applyApproval([], { runtimeId: 'box', approval: pending })
    approvals = applyApproval(approvals, { runtimeId: 'local', approval: pending })
    expect(approvals).toEqual([{ ...pending, runtimeId: 'box' }])
    expect(applyApproval(approvals, { runtimeId: 'local', approval: { ...pending, state: 'answered' } })).toEqual([])
    expect(applyApproval(approvals, {})).toBe(approvals)
  })
})

describe('link lines', () => {
  it('says whether a link answers, since when, or why not', async () => {
    const { linkLabel } = await import('./swarm.js')
    const now = Date.parse('2026-01-01T14:10:00Z')
    expect(linkLabel({ direction: 'outbound', state: 'connected', last_seen_at: '2026-01-01T14:00:00Z' }, now)).toBe('Dials out · connected · seen 10m ago')
    expect(linkLabel({ direction: 'inbound', state: 'connected', last_seen_at: '2026-01-01T14:10:00Z' }, now)).toBe('Accepts · connected · seen just now')
    expect(linkLabel({ direction: 'outbound', state: 'backoff', last_error: 'The machine did not answer.' }, now)).toBe('Dials out · retrying: The machine did not answer.')
    expect(linkLabel({ direction: 'outbound', state: 'revoked' }, now)).toBe('Access revoked. Link the machines again.')
    expect(linkLabel(null)).toBe('Reached through another member')
  })
})

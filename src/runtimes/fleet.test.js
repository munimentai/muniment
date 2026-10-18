import { describe, expect, it } from 'vitest'
import { applyOp, auditRow, dedupeThreads, bootstrapRows, clockTime, credentialRows, historyLine, listsArrived, offloadTargets, offlineScope, openOps, projectOwner, queuedFor, queuedLine, syncLine, threadOwner } from './fleet.js'

const LAPTOP = 'core-laptop'
const STUDIO = 'core-studio'
const BOX = 'core-box'
const POD = 'core-pod'

const member = (runtime_id, runtime_name, form_factor, online = true) => ({ runtime_id, runtime_name, form_factor, online })
const swarm = {
  self: { local: LAPTOP, box: BOX },
  members: [member(LAPTOP, 'Laptop', 'laptop'), member(STUDIO, 'Studio', 'desktop', false), member(BOX, 'Build box', 'server'), member(POD, 'Pod', 'container')],
}
const op = (op_id, state, extra = {}) => ({ op_id, state, operation: 'thread.post', item: { kind: 'thread', id: 't1' }, owner_runtime: STUDIO, owner_name: 'Studio', created_at: `2026-01-01T14:0${op_id.at(-1)}:00Z`, ...extra })

describe('the sync line', () => {
  it('hides while the machine is alone', () => {
    expect(syncLine({ state: 'alone' })).toBeNull()
    expect(syncLine(null)).toBeNull()
  })

  it('reads Synced, Syncing N with open actions and bootstrap classes, and Offline with the last sync time', () => {
    expect(syncLine({ state: 'synced', classes: [] }, [op('o1', 'applied')]).text).toBe('Synced')
    expect(syncLine({ state: 'synced', classes: [] }, [op('o1', 'queued'), op('o2', 'sent'), op('o3', 'failed')]).text).toBe('Syncing 2')
    const bootstrapping = { state: 'bootstrapping', classes: [{ class: 'thread', percent: 100 }, { class: 'history', percent: 30 }, { class: 'artifact', percent: 0 }] }
    expect(syncLine(bootstrapping).text).toBe('Syncing 2')
    const at = new Date(2026, 0, 1, 9, 5).toISOString()
    expect(syncLine({ state: 'offline', last_sync_at: at }).text).toBe('Offline · synced 09:05')
    expect(syncLine({ state: 'offline', last_sync_at: null }).text).toBe('Offline')
    expect(clockTime('nonsense')).toBe('')
  })
})

describe('forwarded actions', () => {
  it('keeps one entry per action, newest first, after each update', () => {
    let ops = applyOp([], { runtimeId: 'local', op: op('o1', 'queued') })
    ops = applyOp(ops, { op: op('o2', 'queued') })
    ops = applyOp(ops, { op: op('o1', 'cancelled') })
    expect(ops.map((entry) => `${entry.op_id}:${entry.state}`)).toEqual(['o2:queued', 'o1:cancelled'])
    expect(openOps(ops).map((entry) => entry.op_id)).toEqual(['o2'])
    expect(applyOp(ops, { op: null })).toBe(ops)
  })

  it('finds the open thread action waiting for an offline owner and words it', () => {
    const offline = op('o1', 'queued', { reason: { code: 'owner_offline' } })
    const waiting = op('o2', 'queued', { reason: { code: 'waiting' } })
    expect(queuedFor([waiting, offline], 't1')).toBe(offline)
    expect(queuedFor([waiting], 't1')).toBeNull()
    expect(queuedFor([op('o3', 'queued', { item: { kind: 'agent', id: 'a1' }, reason: { code: 'owner_offline' } })], 't9', 'a1').op_id).toBe('o3')
    expect(queuedLine(offline)).toBe("Queued. Studio is offline. Sends when it's back.")
  })
})

describe('owners', () => {
  it('names the fleet member that owns a replica thread, and nothing for an own thread', () => {
    const owner = threadOwner({ threadId: 't1', runtimeId: 'local', owner_runtime: STUDIO, replica: true }, swarm)
    expect(owner).toMatchObject({ name: 'Studio', online: false, formFactor: 'desktop' })
    expect(threadOwner({ threadId: 't2', runtimeId: 'local', owner_runtime: LAPTOP }, swarm)).toBeNull()
    expect(threadOwner({ threadId: 't3', runtimeId: 'box', owner_runtime: BOX }, swarm)).toBeNull()
    expect(threadOwner({ threadId: 't4', runtimeId: 'local' }, swarm)).toBeNull()
    expect(threadOwner({ threadId: 't5', runtimeId: 'local', owner_runtime: POD }, swarm)).toMatchObject({ name: 'Pod', kind: 'container' })
  })

  it('lists a thread once, from the machine that owns it', () => {
    const replica = { threadId: 't1', runtimeId: 'local', owner_runtime: BOX, replica: true, title: 'Copy' }
    const owned = { threadId: 't1', runtimeId: 'box', owner_runtime: BOX, title: 'Owner' }
    expect(dedupeThreads([replica, owned, { threadId: 't2', runtimeId: 'local' }], swarm).map((entry) => entry.title ?? entry.threadId)).toEqual(['Owner', 't2'])
    expect(dedupeThreads([owned, replica], swarm)).toEqual([owned])
  })

  it('names the owner of another member\'s project', () => {
    expect(projectOwner({ name: 'Launch', ownerRuntime: STUDIO }, swarm)).toMatchObject({ name: 'Studio' })
    expect(projectOwner({ name: 'Mine', ownerRuntime: LAPTOP }, swarm)).toBeNull()
  })

  it('offers every member the app does not reach as a place for a new thread', () => {
    expect(offloadTargets(swarm, null).map((target) => `${target.coreId}:${target.kind}`)).toEqual([`${POD}:container`, `${STUDIO}:desktop`])
  })
})

describe('offline and bootstrap', () => {
  it('reads the offline scope from the sync status and the pins', () => {
    expect(offlineScope({ offline: { thread_days: 0, attachments: 'all' } }, { offline: { artifacts: 'all' }, pins: [{ class: 'artifact', key: 'report' }] }))
      .toEqual({ threads: 0, attachments: 'all', artifacts: 'all' })
    expect(offlineScope(null, null)).toEqual({ threads: 30, attachments: 'open', artifacts: 'pinned' })
  })

  it('lists each class with its progress and knows when the lists arrived', () => {
    const status = { state: 'bootstrapping', classes: [{ class: 'agent', held: 6, total: 6, percent: 100 }, { class: 'thread', held: 40, total: 412, percent: 9.7 }, { class: 'history', held: 0, total: 0, percent: 0 }] }
    expect(bootstrapRows(status).map((row) => `${row.name}:${row.percent}:${row.done}`)).toEqual(['Agents:100:true', 'Threads:10:false', 'History:0:false'])
    expect(listsArrived(status)).toBe(false)
    expect(listsArrived({ classes: [{ class: 'agent', percent: 100 }, { class: 'thread', percent: 100 }, { class: 'history', percent: 5 }] })).toBe(true)
  })

  it('says where each missing sign-in comes from', () => {
    const credentials = { accounts: [
      { name: 'router:work', label: 'Work', holder: BOX, holder_name: 'Build box', this_machine: false, usable_here: false },
      { name: 'pi:anthropic', holder: STUDIO, holder_name: 'Studio', this_machine: false, usable_here: false },
      { name: 'pi:openai', holder: LAPTOP, this_machine: true, usable_here: true },
    ] }
    expect(credentialRows(credentials, swarm).map((row) => `${row.name}: ${row.line}`)).toEqual(['Work: arriving from Build box', 'pi:anthropic: waiting for a holder'])
    expect(credentialRows({}, swarm)).toEqual([])
    const local = { accounts: [{ name: 'endpoint:custom-omlx', label: 'oMLX', holder: BOX, holder_name: 'Build box', this_machine: false, usable_here: false, shareable: false, reason: 'It runs only on its own computer.' }] }
    expect(credentialRows(local, swarm)).toEqual([{ id: 'endpoint:custom-omlx', name: 'oMLX', line: 'It runs only on its own computer.', online: false }])
  })

  it('describes a replica thread while its history travels', () => {
    expect(historyLine({ status: 'fetching', held_events: 12, total_events: 48 }, 'Studio')).toBe('History arriving from Studio: 12 of 48 events.')
    expect(historyLine({ status: 'fetching', total_events: 0 }, '')).toBe('History arriving.')
    expect(historyLine({ status: 'held' }, 'Studio')).toBe('')
  })
})

describe('the audit log', () => {
  it('names the action, the machines and the detail of each entry', () => {
    const row = auditRow({ id: `${LAPTOP}:0192`, at: '2026-01-01T14:00:00Z', kind: 'role_changed', actor_runtime: LAPTOP, runtime_id: BOX, record: { from: 'full', to: 'worker' } }, swarm)
    expect(row).toMatchObject({ id: `${LAPTOP}:0192`, what: 'Changed a role', machines: 'Laptop → Build box', detail: 'full to worker' })
    expect(auditRow({ at: 'x', kind: 'something_new', runtime_id: 'core-unknown-machine' }, swarm)).toMatchObject({ what: 'something new', machines: 'core-unk', time: '' })
    const named = { members: [{ runtime_id: '018f0000-0000-7000-8000-000000000041', runtime_name: 'Laptop' }] }
    expect(auditRow({ kind: 'sealed', runtime_id: null, actor_runtime: '018f0000-0000-7000-8000-000000000041', record: { secret_id: 'models:018f0000-0000-7000-8000-000000000041', version: 3, recipients: ['a', 'b'], keys: ['pi:anthropic'] } }, named))
      .toMatchObject({ machines: 'Laptop', detail: 'models:Laptop · pi:anthropic · 2 recipients' })
  })

  it('reads each entry in the direction the action went', () => {
    // The owner writes a forwarded action, and `runtime_id` is its sender.
    const op = auditRow({ kind: 'op', actor_runtime: LAPTOP, runtime_id: POD, record: { op_id: 'o1', operation: 'thread.post', actor_device: 'local', origin: POD, result: 'applied', note: null } }, swarm)
    expect(op).toMatchObject({ what: 'Ran a forwarded action', machines: 'Pod → Laptop', detail: 'thread.post · applied' })
    expect(auditRow({ kind: 'opened', actor_runtime: POD, runtime_id: STUDIO, record: { secret_id: `models:${STUDIO}`, recipients: [POD] } }, swarm).machines).toBe('Studio → Pod')
    expect(auditRow({ kind: 'hold_received', actor_runtime: LAPTOP, runtime_id: STUDIO, record: { account: 'pi:anthropic' } }, swarm).machines).toBe('Studio → Laptop')
    expect(auditRow({ kind: 'hold_moved', actor_runtime: STUDIO, runtime_id: LAPTOP, record: { account: 'pi:anthropic' } }, swarm).machines).toBe('Studio → Laptop')
    expect(auditRow({ kind: 'snapshot_served', actor_runtime: LAPTOP, runtime_id: POD, record: { rows: 1, complete: true } }, swarm)).toMatchObject({ machines: 'Laptop → Pod', detail: '1 row' })
    expect(auditRow({ kind: 'vault_token_refused', actor_runtime: LAPTOP, runtime_id: STUDIO, record: { error: 'The account holds five machines.' } }, swarm).detail).toBe('The account holds five machines.')
  })
})

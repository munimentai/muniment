// The fleet as this window shows it: the machine that owns each thread, agent and project, actions queued for
// an offline owner, the offline scope, the bootstrap progress and the audit
// log. Core owns sync, ownership, queues and the log. This module only
// shapes their answers for the window.
import { LOCAL_RUNTIME_ID, machineName as listedMachineName } from './runtimes.js'
import { memberMachine } from '../agents/swarm.js'

// HH:MM in the viewer's clock.
export function clockTime(value) {
  const date = value ? new Date(value) : null
  if (!date || Number.isNaN(date.getTime())) return ''
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

// The forwarded actions after one `fleet-op-updated` event, newest first.
export function applyOp(ops, update) {
  const op = update?.op ?? update
  if (!op?.op_id) return ops
  return [op, ...ops.filter((entry) => entry.op_id !== op.op_id)]
    .sort((a, b) => String(b.created_at ?? '').localeCompare(String(a.created_at ?? '')))
}

// The core runtime id of the machine a window runtime id names.
export function coreRuntime(swarm, runtimeId) {
  return swarm?.self?.[runtimeId || LOCAL_RUNTIME_ID] ?? null
}

// The machine that owns a thread summary, when another fleet member owns it:
// its name, shape and whether it answers. Null for a thread this machine
// owns or one the swarm does not share.
export function threadOwner(summary, swarm, runtimeList = null) {
  const owner = summary?.owner_runtime
  if (!owner) return null
  const holder = coreRuntime(swarm, summary.runtimeId)
  if (!holder || owner === holder) return null
  return memberMachine(swarm, owner, runtimeList)
}

// The machine a reply's receipt names: the fleet member that owns the
// thread, which runs its replies, else the machine that holds the thread.
export function runMachineName(owner, runtimeList, machineId) {
  if (owner?.name) return owner.name
  const id = machineId ?? LOCAL_RUNTIME_ID
  return listedMachineName(runtimeList?.runtimes?.find((runtime) => runtime.id === id) ?? { id })
}

// The machine that owns an agent, when another fleet member is its primary.
export function agentOwner(agent, swarm, runtimeList = null) {
  const owner = agent?.primaryRuntime ?? agent?.primary_runtime
  if (!owner) return null
  const holder = coreRuntime(swarm, agent.runtimeId)
  if (holder && owner === holder && !agent.replica) return null
  return agent.primaryMachine ?? memberMachine(swarm, owner, runtimeList)
}

// The machine that owns a project another member shares.
export function projectOwner(project, swarm, runtimeList = null) {
  const owner = project?.ownerRuntime ?? project?.owner_runtime
  if (!owner || owner === coreRuntime(swarm, LOCAL_RUNTIME_ID)) return null
  return memberMachine(swarm, owner, runtimeList)
}

// The queued action of the open thread that waits for its offline owner.
export function queuedFor(ops, threadId, agentId = null) {
  if (!threadId && !agentId) return null
  return ops.find((op) => op?.state === 'queued'
    && op.reason?.code === 'owner_offline'
    && ((op.item?.kind === 'thread' && op.item.id === threadId) || (agentId && op.item?.kind === 'agent' && op.item.id === agentId))) ?? null
}

// The composer's line for a queued action.
export function queuedLine(op) {
  const name = op?.owner_name || 'Its machine'
  return `Queued. ${name} is offline. Sends when it's back.`
}

// What a replica thread shows while its history travels from its owner.
export function historyLine(fetch, ownerName = '') {
  if (!fetch || fetch.status === 'held') return ''
  const from = ownerName ? ` from ${ownerName}` : ''
  const total = Number(fetch.total_events ?? 0)
  if (!total) return `History arriving${from}.`
  return `History arriving${from}: ${Number(fetch.held_events ?? 0)} of ${total} events.`
}

// The offline scope choices on Settings → Machines → This computer.
export const THREAD_CHOICES = [{ value: 30, label: '30 days' }, { value: 0, label: 'All' }]
export const ATTACHMENT_CHOICES = [{ value: 'open', label: 'On open' }, { value: 'all', label: 'All' }]
export const ARTIFACT_CHOICES = [{ value: 'pinned', label: 'Pinned' }, { value: 'all', label: 'All' }]

// The scope as the page shows it, from `fleet.sync_status.offline` and
// `fleet.pins`. A thread window other than every thread shows as 30 days.
// Projects and skills have no choice: a project's details and membership
// always sync and its files stay on its owner, and every skill syncs.
export function offlineScope(status, pins) {
  const days = Number(status?.offline?.thread_days ?? 30)
  return {
    threads: days === 0 ? 0 : 30,
    attachments: status?.offline?.attachments === 'all' ? 'all' : 'open',
    artifacts: pins?.offline?.artifacts === 'all' ? 'all' : 'pinned',
  }
}

const CLASS_NAMES = {
  agent: 'Agents',
  group: 'Groups',
  thread: 'Threads',
  history: 'History',
  project: 'Projects',
  'project-thread': 'Project threads',
  preference: 'Preferences',
  permission: 'Permissions',
  memory: 'Memory',
  artifact: 'Artifacts',
  extension: 'Skills',
  account: 'Model accounts',
  route: 'Routing',
  'decision-model': 'Decision models',
  search: 'Search',
  retention: 'Retention',
  audit: 'Log',
}

export function className(name) {
  return CLASS_NAMES[name] ?? String(name ?? '').replace(/[-_]/g, ' ').replace(/^./, (letter) => letter.toUpperCase())
}

// The bootstrap rows: each class with its count and percent. The lists come
// first, so the app is usable once they reach 100.
export function bootstrapRows(status) {
  return (status?.classes ?? []).map((entry) => {
    const percent = Math.max(0, Math.min(100, Math.round(Number(entry.percent ?? 0))))
    return { id: entry.class, name: className(entry.class), held: Number(entry.held ?? 0), total: Number(entry.total ?? 0), percent, done: percent >= 100 }
  })
}

const LISTS = new Set(['agent', 'group', 'thread', 'project'])

// Whether every list class has arrived, so the app can be used.
export function listsArrived(status) {
  return (status?.classes ?? []).filter((entry) => LISTS.has(entry.class)).every((entry) => Number(entry.percent ?? 0) >= 100)
}

// One line per model account this machine cannot use yet: who it arrives
// from, that no member holding it answers, or core's reason sharing never
// carries it, such as a server only its own computer reaches.
export function credentialRows(credentials, swarm) {
  return (credentials?.accounts ?? [])
    .filter((account) => !account.this_machine && !account.usable_here)
    .map((account) => {
      const holder = swarm?.members?.find((member) => member.runtime_id === account.holder)
      const online = !!holder?.online
      const from = account.holder_name || holder?.runtime_name
      const stays = account.shareable === false
      return {
        id: account.name,
        name: account.label || account.name,
        line: stays ? account.reason || 'stays on its own machine' : online && from ? `arriving from ${from}` : 'waiting for a holder',
        online: online && !stays,
      }
    })
}

const AUDIT_LINES = {
  sealed: 'Sealed a secret',
  opened: 'Opened a secret',
  revoked: 'Removed a machine',
  vault_token_issued: 'Issued a vault token',
  vault_token_revoked: 'Revoked a vault token',
  vault_token_refused: 'Vault token refused',
  hold_moved: 'Moved a sign-in',
  hold_received: 'Took a sign-in',
  role_changed: 'Changed a role',
  op: 'Ran a forwarded action',
  snapshot_served: 'Sent a snapshot',
}

// The kinds whose entry `runtime_id` names the machine the action came from:
// the sender of a forwarded action, the owner of an opened secret, and the
// last holder of a sign-in. Every other kind names the machine it acted on.
const FROM_MACHINE = new Set(['op', 'opened', 'hold_received'])

// The most entries one `fleet.audit` answers.
export const AUDIT_LIMIT = 500

// The machine name behind a core runtime id, for the log.
function machineName(swarm, runtimeId) {
  if (!runtimeId) return ''
  const member = swarm?.members?.find((entry) => entry.runtime_id === runtimeId)
  if (member) return memberMachine(swarm, runtimeId).name
  return `${String(runtimeId).slice(0, 8)}`
}

const RUNTIME_ID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g

// One `fleet.audit` entry as a log row: when, what, the machines it moved
// between in the direction it went, and the record's detail. Core keys each
// entry `<writer>:<uuidv7>`, and `actor_runtime` is the writer.
export function auditRow(entry, swarm) {
  const record = entry?.record ?? {}
  const rows = Number.isInteger(record.rows) ? `${record.rows} ${record.rows === 1 ? 'row' : 'rows'}` : ''
  const detail = [
    record.operation,
    record.secret_id,
    record.account,
    record.from && record.to ? `${record.from} to ${record.to}` : '',
    Array.isArray(record.keys) ? record.keys.join(', ') : '',
    Array.isArray(record.recipients) ? `${record.recipients.length} ${record.recipients.length === 1 ? 'recipient' : 'recipients'}` : '',
    rows,
    record.result,
    record.note,
    record.error,
  ].filter((part) => typeof part === 'string' && part).join(' · ')
    .replace(RUNTIME_ID, (id) => (swarm?.members?.some((member) => member.runtime_id === id) ? machineName(swarm, id) : id))
  const actor = machineName(swarm, entry?.actor_runtime)
  const other = machineName(swarm, entry?.runtime_id)
  const [from, to] = FROM_MACHINE.has(entry?.kind) ? [other, actor] : [actor, other]
  return {
    id: entry?.id ?? `${entry?.at}:${entry?.kind}`,
    at: entry?.at ?? '',
    time: auditTime(entry?.at),
    what: AUDIT_LINES[entry?.kind] ?? String(entry?.kind ?? '').replace(/_/g, ' '),
    machines: from && to && from !== to ? `${from} → ${to}` : actor || other,
    detail,
  }
}

function auditTime(value) {
  const date = value ? new Date(value) : null
  if (!date || Number.isNaN(date.getTime())) return ''
  const day = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
  return `${day} ${clockTime(value)}`
}

// The fleet members a new thread can start on, beyond the machines this app
// reaches directly: each with its core id, name, shape and whether it
// answers. Containers count like any member.
export function offloadTargets(swarm, runtimeList) {
  const reached = new Set(Object.values(swarm?.self ?? {}))
  return (swarm?.members ?? [])
    .filter((member) => !reached.has(member.runtime_id) && !member.fleet?.removed)
    .map((member) => ({ ...memberMachine(swarm, member.runtime_id, runtimeList), coreId: member.runtime_id }))
    .sort((a, b) => String(a.name).localeCompare(String(b.name)))
}

// One summary per thread. A thread the app sees both on its owner and as a
// replica on another machine it reaches lists once, from its owner.
export function dedupeThreads(summaries, swarm) {
  const kept = new Map()
  for (const summary of summaries ?? []) {
    const known = kept.get(summary.threadId)
    if (!known) { kept.set(summary.threadId, summary); continue }
    // A runtime that lists a thread it runs marks it no replica, even
    // before the app knows that runtime's fleet id.
    const owns = (entry) => !!entry.owner_runtime && (entry.owner_runtime === coreRuntime(swarm, entry.runtimeId) || !entry.replica)
    if (!owns(known) && owns(summary)) kept.set(summary.threadId, summary)
  }
  return [...kept.values()]
}

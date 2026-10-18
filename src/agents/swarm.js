// The swarm as this window sees it: the members, the agent directory, the
// groups and the pending approvals that each machine the app reaches
// answers. Core owns links, sync, moves and approval routing. This module
// gathers the answers, names the machine behind each core runtime id, and
// picks the runtime a request goes to.
import { LOCAL_RUNTIME_ID, machineName } from '../runtimes/runtimes.js'
import { agentRequest, agentRuntimes } from './agent-runtime.js'

export function emptySwarm() {
  return {
    // The core runtime id of each machine the app reaches, by its window id.
    self: {},
    // Each machine's own `swarm.members` answer, by its window id.
    views: {},
    members: [],
    agents: [],
    groups: [],
    approvals: [],
    absence: { paused: false, paused_at: null, last_client_at: null },
  }
}

const newer = (a, b) => String(a ?? '') > String(b ?? '')

// The directory entry that wins: the highest epoch, then the newest update.
function winner(current, next) {
  if (!current) return next
  if ((next.epoch ?? 0) !== (current.epoch ?? 0)) return (next.epoch ?? 0) > (current.epoch ?? 0) ? next : current
  return newer(next.updated_at, current.updated_at) ? next : current
}

// Merges each machine's answers into one view. `answers` holds, per machine,
// `{ runtimeId, members, agents, groups, approvals }`, each the answer body
// or null when that machine refused or did not answer.
export function mergeSwarm(answers) {
  const swarm = emptySwarm()
  const members = new Map()
  const agents = new Map()
  const groups = new Map()
  const approvals = new Map()
  for (const answer of answers) {
    const { runtimeId } = answer
    const view = answer.members
    if (view?.self) {
      swarm.self[runtimeId] = view.self
      swarm.views[runtimeId] = view
      if (view.absence?.paused) swarm.absence = { ...view.absence }
      else if (!swarm.absence.paused && newer(view.absence?.last_client_at, swarm.absence.last_client_at)) swarm.absence = { ...view.absence }
      for (const member of view.members ?? []) {
        const known = members.get(member.runtime_id)
        const { self: _self, link: _link, ...entry } = member
        members.set(member.runtime_id, known
          ? { ...known, online: known.online || member.online, heartbeat_at: newer(member.heartbeat_at, known.heartbeat_at) ? member.heartbeat_at : known.heartbeat_at }
          : entry)
      }
    }
    for (const entry of answer.agents?.agents ?? []) {
      agents.set(entry.agent_id, winner(agents.get(entry.agent_id), { ...entry, runtimeId: agents.get(entry.agent_id)?.runtimeId ?? runtimeId }))
    }
    for (const group of answer.groups?.groups ?? []) {
      const known = groups.get(group.group_id)
      if (!known || newer(group.updated_at, known.updated_at)) groups.set(group.group_id, { ...group, runtimeId: known?.runtimeId ?? runtimeId })
    }
    for (const approval of answer.approvals?.approvals ?? []) {
      if (!approvals.has(approval.approval_id)) approvals.set(approval.approval_id, { ...approval, runtimeId })
    }
  }
  swarm.members = [...members.values()]
  swarm.agents = [...agents.values()].filter((entry) => !entry.deleted)
  swarm.groups = [...groups.values()].filter((group) => !group.deleted)
  swarm.approvals = [...approvals.values()].filter((approval) => approval.state === 'pending')
  // A group thread and an answer go first to the machine that owns it, when
  // the app reaches that machine.
  for (const group of swarm.groups) group.runtimeId = windowRuntime(swarm, group.owner_runtime) ?? group.runtimeId
  for (const approval of swarm.approvals) approval.runtimeId = windowRuntime(swarm, approval.runtime_id) ?? approval.runtimeId
  return swarm
}

// Asks every machine the app reaches for its swarm. A machine without a
// swarm refuses, and its answers count as empty.
export async function listSwarm(tauri, runtimeList) {
  const targets = agentRuntimes(runtimeList)
  const ask = async (runtime, operation) => {
    try { return await agentRequest(tauri, runtime.id, operation) } catch { return null }
  }
  const answers = await Promise.all(targets.map(async (runtime) => {
    const [members, agents, groups, approvals] = await Promise.all(['swarm.members', 'swarm.agents', 'group.list', 'agent.approvals'].map((operation) => ask(runtime, operation)))
    return { runtimeId: runtime.local ? LOCAL_RUNTIME_ID : runtime.id, members, agents, groups, approvals }
  }))
  return mergeSwarm(answers)
}

// The window id of the machine with core runtime id `coreId`, when the app
// reaches it.
export function windowRuntime(swarm, coreId) {
  if (!coreId) return null
  return Object.entries(swarm?.self ?? {}).find(([, id]) => id === coreId)?.[0] ?? null
}

// The machine behind a core runtime id, shaped for `MachineMark`: its name,
// its form factor and whether it answers.
export function memberMachine(swarm, coreId, runtimeList = null) {
  const member = swarm?.members?.find((entry) => entry.runtime_id === coreId)
  const reached = windowRuntime(swarm, coreId)
  const runtime = reached ? runtimeList?.runtimes?.find((entry) => entry.id === reached) : null
  if (!member) {
    return runtime
      ? { ...runtime, online: runtime.local || runtime.status === 'online' }
      : { id: coreId, name: 'Unknown machine', kind: 'server', online: false }
  }
  // A machine this window reaches keeps the kind and form factor its runtime
  // reports, so it draws the same icon here as on the Machines page.
  const formFactor = member.form_factor
  const kind = formFactor === 'container' ? 'container' : reached === LOCAL_RUNTIME_ID ? 'local' : runtime?.kind ?? 'desktop'
  return {
    id: coreId,
    name: reached === LOCAL_RUNTIME_ID ? 'This computer' : machineName({ name: member.runtime_name || runtime?.name, kind }),
    kind,
    formFactor: runtime ? runtime.formFactor : formFactor,
    online: !!member.online,
  }
}

// Every agent the window lists: each agent a machine answered for, then each
// agent only the swarm directory names. Each carries its primary machine,
// whether that machine answers, and `replica` when the app sees it only
// through a member's synced copy.
export function swarmRoster(listing, swarm, runtimeList = null) {
  const listed = new Set()
  const roster = (listing?.agents ?? []).map((agent) => {
    listed.add(agent.id)
    const entry = swarm?.agents?.find((item) => item.agent_id === agent.id)
    const primary = agent.primary_runtime ?? entry?.primary_runtime ?? swarm?.self?.[agent.runtimeId ?? LOCAL_RUNTIME_ID] ?? null
    return { ...agent, primaryRuntime: primary, primaryThread: entry?.primary_thread ?? null, replica: false }
  })
  for (const entry of swarm?.agents ?? []) {
    if (listed.has(entry.agent_id)) continue
    roster.push({
      id: entry.agent_id,
      name: entry.name,
      label: entry.label ?? '',
      avatar: entry.avatar ?? null,
      instructions: '',
      routines: [],
      epoch: entry.epoch ?? 0,
      primary_runtime: entry.primary_runtime,
      primaryRuntime: entry.primary_runtime,
      primaryThread: entry.primary_thread ?? null,
      runtimeId: entry.runtimeId ?? LOCAL_RUNTIME_ID,
      replica: true,
      move_suggested: !!entry.move_suggested,
    })
  }
  return roster.map((agent) => {
    const machine = agent.primaryRuntime ? memberMachine(swarm, agent.primaryRuntime, runtimeList) : null
    return { ...agent, primaryMachine: machine, online: machine ? machine.online : true }
  })
}

// Which agent each replicated primary thread belongs to.
export function swarmThreads(swarm) {
  const threads = {}
  for (const entry of swarm?.agents ?? []) if (entry.primary_thread) threads[entry.primary_thread] = entry.agent_id
  return threads
}

// Whether a message to the open thread goes through `thread.post`: a group
// thread, an agent thread this machine holds as a replica of another
// member's, or an ordinary thread another member owns. Core sends a post on
// to the owner. Answers the request, or null for an ordinary run.
export function postTarget({ threadId, threadRuntime, groups = [], agent = null, swarm, owner = null }) {
  if (!threadId) return null
  const runtime = threadRuntime || LOCAL_RUNTIME_ID
  const group = groups.find((entry) => entry.thread_id === threadId)
  if (group) return { runtime, body: { thread_id: threadId }, group }
  const holder = swarm?.self?.[runtime]
  const primary = agent ? agent.primaryRuntime ?? agent.primary_runtime : owner
  if (holder && primary && primary !== holder) return { runtime, body: { thread_id: threadId }, group: null }
  return null
}

// Links two machines this app administers: the first issues a swarm
// invitation and the second redeems it. The link never leaves the app.
export async function linkMachines(tauri, from, to) {
  if (!from || !to || from === to) throw new Error('Choose two different machines.')
  const invitation = await agentRequest(tauri, from, 'swarm.invite', {})
  const joined = await agentRequest(tauri, to, 'swarm.join', { link: invitation.link })
  return joined?.member ?? null
}

// The members a machine links to directly, from its own view.
export function directLinks(view) {
  return (view?.members ?? []).filter((member) => !member.self && member.link)
}

// What a move of `agent` to the member `to` asks for. The machine that holds
// the agent sends the move, or the request, to its primary. A forced move
// runs on the target only and only while the primary does not answer, so the
// app must reach the target.
export function moveRequest({ agent, to, swarm, force = false }) {
  const from = agent?.primaryRuntime ?? agent?.primary_runtime ?? null
  if (!to || to === from) return { error: 'Choose another machine.' }
  const primary = from ? swarm?.members?.find((member) => member.runtime_id === from) : null
  const primaryOnline = primary ? !!primary.online : true
  const body = { id: agent.id, to, force: !!force }
  if (force) {
    if (primaryOnline) return { error: 'The primary machine answers. Move without forcing.', primaryOnline }
    const runtime = windowRuntime(swarm, to)
    if (!runtime) return { error: 'Pair this device with the target machine to force the move.', primaryOnline }
    return { runtime, body, primaryOnline, forced: true }
  }
  if (!primaryOnline) return { error: 'The primary machine does not answer. Force the move or wait for it.', primaryOnline, canForce: !!windowRuntime(swarm, to) }
  return { runtime: agent.runtimeId ?? windowRuntime(swarm, from) ?? LOCAL_RUNTIME_ID, body, primaryOnline }
}

// The line a move answer earns.
export function moveOutcome(status, machineName) {
  if (status === 'moved') return `Moved to ${machineName}.`
  if (status === 'requested') return `Move to ${machineName} requested. The primary hands over its work first.`
  if (status === 'forced') return `Forced the move to ${machineName}. The old primary steps down when it returns.`
  return ''
}

// The pending approvals after one `agent-approval-pending` or
// `agent-approval-resolved` event.
export function applyApproval(approvals, update) {
  const approval = update?.approval
  if (!approval?.approval_id) return approvals
  const rest = approvals.filter((entry) => entry.approval_id !== approval.approval_id)
  if (approval.state !== 'pending') return rest
  const known = approvals.find((entry) => entry.approval_id === approval.approval_id)
  return [...rest, { ...approval, runtimeId: known?.runtimeId ?? update.runtimeId ?? LOCAL_RUNTIME_ID }]
    .sort((a, b) => String(a.created_at).localeCompare(String(b.created_at)))
}

// One line for a link: whether it answers and since when, or why not.
export function linkLabel(link, now = Date.now()) {
  if (!link) return 'Through another machine'
  const seen = Date.parse(link.last_seen_at ?? '')
  const minutes = Number.isFinite(seen) ? Math.max(0, Math.round((now - seen) / 60_000)) : null
  const ago = minutes === null ? '' : minutes < 1 ? ' · now' : minutes < 60 ? ` · ${minutes}m ago` : ` · ${Math.round(minutes / 60)}h ago`
  if (link.state === 'connected') return `Connected${ago}`
  if (link.state === 'revoked') return 'Access revoked. Join the machines again.'
  return `Retrying${link.last_error ? `: ${link.last_error}` : ''}`
}

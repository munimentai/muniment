// Agents on every machine the app reaches. Each request goes through
// `agent_request` to one runtime's core agent operations, and the runtime
// owns the agents, routines and runs. This module only gathers the answers.
import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'
import { runActive } from '../lib/agent-labels.js'

export function agentRequest(tauri, runtime, operation, body = {}) {
  return tauri.invoke('agent_request', { runtime: runtime || LOCAL_RUNTIME_ID, operation, body })
}

// The lines a runtime answers while it starts, is busy or is slow. A read
// that gets one asks again.
export const UNREACHABLE_LINE = 'Muniment cannot reach its background service.'
const TRANSIENT_LINES = [UNREACHABLE_LINE, 'Muniment is busy with another request.', 'The background service did not answer in time.']
export function transientAgentError(error) {
  const text = String(error?.message ?? error ?? '')
  return TRANSIENT_LINES.some((line) => text.startsWith(line))
}

// How long a read waits before each new ask after a transient answer.
export const READ_RETRY_DELAYS = [250, 500, 1000, 2000, 4000]

// One read-only agent operation that asks again after a transient answer.
// Only reads retry, so a busy answer never repeats a change.
export async function agentRead(tauri, runtime, operation, body = {}, delays = READ_RETRY_DELAYS) {
  for (let attempt = 0; ; attempt += 1) {
    try {
      return await agentRequest(tauri, runtime, operation, body)
    } catch (error) {
      if (attempt >= delays.length || !transientAgentError(error)) throw error
      await new Promise((resolve) => setTimeout(resolve, delays[attempt]))
    }
  }
}

// One change to an agent through core's `agent.save` on its machine. It reads
// the agent first, so edits made elsewhere stay, and a null schedule leaves
// the routines as they are. Core validates the change and refuses with its reason.
export async function saveAgentPatch(tauri, agent, patch) {
  const { agent: latest } = await agentRequest(tauri, agent.runtimeId, 'agent.read', { id: agent.id })
  return agentRequest(tauri, agent.runtimeId, 'agent.save', { agent: { ...latest, ...patch, schedule: null } })
}

// This computer, then each paired machine that answers.
export function agentRuntimes(list) {
  const runtimes = list?.runtimes?.length ? list.runtimes : [{ id: LOCAL_RUNTIME_ID, local: true }]
  return runtimes.filter((runtime) => runtime.local || runtime.status === 'online')
}

// `answered` names each machine whose agents the listing holds, so a
// machine that has not answered yet is told apart from one with no agents.
export function emptyListing() {
  return { agents: [], primaryThreads: {}, threads: {}, latestRuns: {}, errors: [], unreachable: [], answered: [] }
}

// Every thread of each listed agent, from core's `agent.threads` on the
// agent's own machine: `{ [agentId]: [{ thread_id, title, updated_at,
// primary }] }`, newest first. A machine that refuses the operation leaves
// that agent with its primary thread alone.
export async function listAgentThreads(tauri, agents) {
  const answers = await Promise.allSettled(agents.map((agent) => agentRequest(tauri, agent.runtimeId, 'agent.threads', { id: agent.id })))
  const threads = {}
  answers.forEach((answer, index) => {
    if (answer.status === 'fulfilled') threads[agents[index].id] = answer.value?.threads ?? []
  })
  return threads
}

// One listing across machines. Each agent carries the `runtimeId` it came
// from. A paired machine that does not answer is named in `unreachable`, and
// only a failure of this computer's runtime fails the whole listing.
// A machine that does not answer this time, such as a busy runtime, keeps
// the agents the `previous` listing holds for it, so its agents never read
// as another machine's synced copies while it is busy.
export async function listAgents(tauri, runtimeList, previous = null) {
  const targets = agentRuntimes(runtimeList)
  const answers = await Promise.allSettled(targets.map((runtime) => agentRequest(tauri, runtime.id, 'agent.list')))
  const listing = emptyListing()
  let localError = null
  answers.forEach((answer, index) => {
    const runtimeId = targets[index].local ? LOCAL_RUNTIME_ID : targets[index].id
    if (answer.status === 'rejected') {
      if (runtimeId === LOCAL_RUNTIME_ID) localError = answer.reason
      else listing.unreachable.push(runtimeId)
      if (!previous?.answered?.includes(runtimeId)) return
      const kept = previous.agents.filter((agent) => (agent.runtimeId ?? LOCAL_RUNTIME_ID) === runtimeId)
      const ids = new Set(kept.map((agent) => agent.id))
      listing.agents.push(...kept)
      for (const [id, thread] of Object.entries(previous.primaryThreads ?? {})) if (ids.has(id)) listing.primaryThreads[id] = thread
      for (const [id, run] of Object.entries(previous.latestRuns ?? {})) if (ids.has(id)) listing.latestRuns[id] = run
      listing.answered.push(runtimeId)
      return
    }
    const body = answer.value ?? {}
    for (const agent of body.agents ?? []) listing.agents.push({ ...agent, runtimeId })
    Object.assign(listing.primaryThreads, body.primary_threads ?? {})
    Object.assign(listing.latestRuns, Object.fromEntries(Object.entries(body.latest_runs ?? {}).map(([id, run]) => [id, { ...run, runtimeId }])))
    for (const error of body.errors ?? []) listing.errors.push({ ...error, runtimeId })
    listing.answered.push(runtimeId)
  })
  if (localError && answers.every((answer) => answer.status === 'rejected')) throw localError
  listing.threads = await listAgentThreads(tauri, listing.agents)
  return listing
}

// Which agent each known thread belongs to: every thread `agent.threads`
// names, its primary conversation and the thread of its latest run.
export function agentThreads(listing) {
  const threads = {}
  for (const [agentId, list] of Object.entries(listing?.threads ?? {})) for (const thread of list) if (thread?.thread_id) threads[thread.thread_id] = agentId
  for (const [agentId, threadId] of Object.entries(listing?.primaryThreads ?? {})) threads[threadId] = agentId
  for (const run of Object.values(listing?.latestRuns ?? {})) if (run?.thread_id && !threads[run.thread_id]) threads[run.thread_id] = run.agent_id
  return threads
}

const newestFirst = (a, b) => String(b.updated_at ?? '').localeCompare(String(a.updated_at ?? ''))

// The newest runs of one agent on its machine, or of every agent on every
// machine that answers.
export async function listRuns(tauri, runtimeList, { agentId = null, runtimeId = null, limit = 50 } = {}) {
  const targets = runtimeId ? [{ id: runtimeId, local: runtimeId === LOCAL_RUNTIME_ID }] : agentRuntimes(runtimeList)
  const body = agentId ? { agent_id: agentId, limit } : { limit }
  const answers = await Promise.allSettled(targets.map((runtime) => agentRequest(tauri, runtime.id, 'agent.runs', body)))
  const runs = answers.flatMap((answer, index) => answer.status === 'fulfilled'
    ? (answer.value?.runs ?? []).map((run) => ({ ...run, runtimeId: targets[index].local ? LOCAL_RUNTIME_ID : targets[index].id }))
    : [])
  if (runs.length === 0 && answers.length && answers.every((answer) => answer.status === 'rejected')) throw answers[0].reason
  return runs.sort(newestFirst)
}

// The run list after one `agent-run-updated` event, `{ runtimeId, run }`.
export function applyRunUpdate(runs, update, limit = 200) {
  if (!update?.run?.id) return runs
  const run = { ...update.run, runtimeId: update.runtimeId || LOCAL_RUNTIME_ID }
  return [run, ...runs.filter((entry) => !(entry.id === run.id && entry.runtimeId === run.runtimeId))].sort(newestFirst).slice(0, limit)
}

// A fresh listing, with each agent's latest run kept from `held` where that
// run is newer. An `agent.list` that started before a run's event answers
// with the run as it was then, so a card would show a settled run as running
// until the next listing. The held run stays when the listing names the same
// run still active after it settled, or an older run. A listing that names
// no run for the agent stands.
export function keepNewerRuns(held, fresh) {
  const latestRuns = { ...(fresh?.latestRuns ?? {}) }
  const at = (run) => { const time = Date.parse(run?.updated_at ?? ''); return Number.isFinite(time) ? time : -Infinity }
  for (const agent of fresh?.agents ?? []) {
    const kept = held?.latestRuns?.[agent.id]
    const listed = latestRuns[agent.id]
    if (!kept || !listed || (kept.runtimeId ?? LOCAL_RUNTIME_ID) !== (agent.runtimeId ?? LOCAL_RUNTIME_ID)) continue
    const settled = kept.id === listed.id && !runActive(kept.status) && runActive(listed.status)
    if (settled || at(kept) > at(listed)) latestRuns[agent.id] = kept
  }
  return { ...fresh, latestRuns }
}

// The machine an agent lives on, for its badge.
export function agentMachine(runtimeList, agent) {
  const id = agent?.runtimeId ?? LOCAL_RUNTIME_ID
  return runtimeList?.runtimes?.find((runtime) => runtime.id === id) ?? (id === LOCAL_RUNTIME_ID ? { id, local: true, kind: 'local', name: 'This computer' } : { id, kind: 'server', name: 'Paired machine' })
}

// Agents on every machine the app reaches. Each request goes through
// `agent_request` to one runtime's core agent operations, and the runtime
// owns the agents, routines and runs. This module only gathers the answers.
import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

export function agentRequest(tauri, runtime, operation, body = {}) {
  return tauri.invoke('agent_request', { runtime: runtime || LOCAL_RUNTIME_ID, operation, body })
}

// This computer, then each paired machine that answers.
export function agentRuntimes(list) {
  const runtimes = list?.runtimes?.length ? list.runtimes : [{ id: LOCAL_RUNTIME_ID, local: true }]
  return runtimes.filter((runtime) => runtime.local || runtime.status === 'online')
}

export function emptyListing() {
  return { agents: [], primaryThreads: {}, threads: {}, latestRuns: {}, errors: [], unreachable: [] }
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
export async function listAgents(tauri, runtimeList) {
  const targets = agentRuntimes(runtimeList)
  const answers = await Promise.allSettled(targets.map((runtime) => agentRequest(tauri, runtime.id, 'agent.list')))
  const listing = emptyListing()
  let localError = null
  answers.forEach((answer, index) => {
    const runtimeId = targets[index].local ? LOCAL_RUNTIME_ID : targets[index].id
    if (answer.status === 'rejected') {
      if (runtimeId === LOCAL_RUNTIME_ID) localError = answer.reason
      else listing.unreachable.push(runtimeId)
      return
    }
    const body = answer.value ?? {}
    for (const agent of body.agents ?? []) listing.agents.push({ ...agent, runtimeId })
    Object.assign(listing.primaryThreads, body.primary_threads ?? {})
    Object.assign(listing.latestRuns, Object.fromEntries(Object.entries(body.latest_runs ?? {}).map(([id, run]) => [id, { ...run, runtimeId }])))
    for (const error of body.errors ?? []) listing.errors.push({ ...error, runtimeId })
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

// The machine an agent lives on, for its badge.
export function agentMachine(runtimeList, agent) {
  const id = agent?.runtimeId ?? LOCAL_RUNTIME_ID
  return runtimeList?.runtimes?.find((runtime) => runtime.id === id) ?? (id === LOCAL_RUNTIME_ID ? { id, local: true, kind: 'local', name: 'This computer' } : { id, kind: 'server', name: 'Paired machine' })
}

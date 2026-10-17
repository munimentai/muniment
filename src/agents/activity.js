// What each agent is doing now, for its avatar. Core derives an agent's
// activity from its runs and the gate a run waits at, lists it in
// `agent.list` and the swarm directory, and sends `agent.activity` when it
// changes. These helpers only pick the newest report and name it.

const STATES = new Set(['idle', 'running', 'waiting', 'error'])

export function activityState(activity) {
  return STATES.has(activity?.state) ? activity.state : 'idle'
}

// The line the avatar shows on hover.
export function activityLabel(activity) {
  const state = activityState(activity)
  const action = String(activity?.action ?? '').trim()
  if (state === 'running') return action ? `Running: ${action}` : 'Running'
  if (state === 'waiting') return action ? `Waiting for you: ${action}` : 'Waiting for you'
  if (state === 'error') return action ? `Last run failed: ${action}` : 'Last run failed'
  return 'Idle'
}

const at = (value) => {
  const time = Date.parse(value ?? '')
  return Number.isFinite(time) ? time : -Infinity
}

// The live map after one `agent-activity` event, `{ runtimeId, update }`,
// where `update` is core's `{ agent_id, runtime_id, activity }`. A report
// older than the one held, by `since`, is dropped.
export function applyActivity(live, event) {
  const update = event?.update
  if (!update?.agent_id || !update.activity) return live
  const held = live[update.agent_id]
  if (held && at(held.since) > at(update.activity.since)) return live
  return { ...live, [update.agent_id]: { ...update.activity } }
}

// An agent's activity: the live event, else what its machine listed, else
// what the swarm directory gossiped. Undefined when no source reports it.
export function agentActivity(agent, live = {}, directory = []) {
  if (!agent) return undefined
  const entry = directory.find((item) => item.agent_id === agent.id)
  return live[agent.id] ?? agent.activity ?? entry?.activity ?? undefined
}

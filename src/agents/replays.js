// The failure replay library. Core saves the input of each failed run in
// the agent's folder, newest 20, with secrets removed, lists them through
// `agent.replays` and queues a new run of one through `agent.replay`. These
// helpers ask the agent's machine and name what came back.
import { agentRequest } from './agent-runtime.js'
import { triggerLabel } from '../lib/agent-labels.js'

export async function loadReplays(tauri, agent) {
  try {
    const answer = await agentRequest(tauri, agent.runtimeId, 'agent.replays', { id: agent.id })
    return { replays: answer?.replays ?? [], version: answer?.agent_version ?? null, error: '' }
  } catch (failure) {
    return { replays: [], version: null, error: String(failure?.message ?? failure) }
  }
}

// Queues a replay of `replay` against the agent as it is now. Answers the
// new run, placed on the agent's machine, and the replay with that run in
// `replayed_by`.
export async function replayRun(tauri, agent, replay) {
  const answer = await agentRequest(tauri, agent.runtimeId, 'agent.replay', { id: agent.id, replay_id: replay.replay_id })
  return { run: answer?.run ? { ...answer.run, runtimeId: agent.runtimeId ?? 'local' } : null, replay: answer?.replay ?? replay }
}

// What started the failed run: its trigger and, for a routine, its name.
export function replaySource(replay, agent) {
  const kind = triggerLabel(replay?.trigger?.kind) || 'Run'
  const routine = (agent?.routines ?? []).find((entry) => entry.id === replay?.trigger?.routine_id)
  return routine ? `${kind} · ${routine.name}` : kind
}

// The notes a replay carries: the agent changed since it failed, secrets
// were removed from its input, and how often it ran again.
export function replayNotes(replay) {
  const notes = []
  if (replay?.definition_changed) notes.push('The agent changed since this failure. A replay uses it as it is now.')
  if (replay?.redacted) notes.push('Secrets in the input were removed.')
  const count = replay?.replayed_by?.length ?? 0
  if (count) notes.push(`Replayed ${count} ${count === 1 ? 'time' : 'times'}.`)
  return notes
}

// The run a replay queued, once Activity knows it: it gains its thread when
// it starts.
export function replayedRun(runs = [], run) {
  if (!run?.id) return null
  return runs.find((entry) => entry.id === run.id && (entry.runtimeId ?? 'local') === (run.runtimeId ?? 'local')) ?? run
}

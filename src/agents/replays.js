// The failure replay library. Core saves the input of each failed run in
// the agent's folder, newest 20, with secrets removed, lists them through
// `agent.replays` and queues a new run of one through `agent.replay`. These
// helpers ask the agent's machine and name what came back.
import { agentRequest } from './agent-runtime.js'

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

// This computer's fleet state for the whole window: the actions core
// forwarded to their owners. It reads them once, then follows
// `fleet-op-updated`. A runtime without a swarm refuses the read, and the
// list stays empty.
import { agentRequest } from '../agents/agent-runtime.js'
import { LOCAL_RUNTIME_ID } from './runtimes.js'
import { applyOp } from './fleet.js'

export function createFleet(tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args)) {
  const fleet = $state({ ops: [], error: '' })
  const ask = (operation, body = {}) => agentRequest(tauri, LOCAL_RUNTIME_ID, operation, body)

  async function refresh() {
    try { fleet.ops = (await ask('fleet.ops', { state: 'open' }))?.ops ?? [] } catch (_) {}
  }

  // Cancels an action that waits for its owner. Core refuses one that
  // already left this machine and says why.
  async function cancel(opId) {
    fleet.error = ''
    try {
      const answer = await ask('fleet.op_cancel', { op_id: opId })
      if (answer?.op) fleet.ops = applyOp(fleet.ops, { op: answer.op })
      return answer?.op ?? null
    } catch (failure) {
      fleet.error = String(failure?.message ?? failure)
      return null
    }
  }

  // Notes the answer of a forwarded call, so the composer shows it at once.
  function forwarded(answer) {
    if (!answer?.forwarded || !answer.op_id) return
    void ask('fleet.op_status', { op_id: answer.op_id }).then((reply) => {
      if (reply?.op) fleet.ops = applyOp(fleet.ops, { op: reply.op })
    }).catch(() => {})
  }

  function start() {
    let alive = true
    const stops = []
    void refresh()
    for (const [event, handle] of [
      ['fleet-op-updated', (payload) => { fleet.ops = applyOp(fleet.ops, payload) }],
      // A member that joins or leaves changes what the fleet holds.
      ['swarm-changed', () => { void refresh() }],
    ]) {
      void Promise.resolve(listen?.(event, ({ payload }) => handle(payload)))
        .then((unlisten) => { if (alive) stops.push(unlisten); else unlisten?.() })
        .catch(() => {})
    }
    return () => { alive = false; for (const stop of stops) stop?.() }
  }

  return { state: fleet, refresh, cancel, forwarded, start, ask }
}

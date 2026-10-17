// This computer's fleet state for the whole window: core's sync status, the
// actions it forwarded and its model accounts. It reads them once, then
// follows `fleet-sync-changed` and `fleet-op-updated`. A runtime without a
// swarm refuses the reads, and the state stays alone.
import { agentRequest } from '../agents/agent-runtime.js'
import { LOCAL_RUNTIME_ID } from './runtimes.js'
import { applyOp, emptySyncStatus } from './fleet.js'

export function createFleet(tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args)) {
  const fleet = $state({ status: emptySyncStatus(), ops: [], credentials: null, error: '' })
  const ask = (operation, body = {}) => agentRequest(tauri, LOCAL_RUNTIME_ID, operation, body)

  async function refresh() {
    const [status, ops, credentials] = await Promise.allSettled([ask('fleet.sync_status'), ask('fleet.ops', { state: 'open' }), ask('fleet.credentials')])
    if (status.status === 'fulfilled' && status.value?.state) fleet.status = status.value
    if (ops.status === 'fulfilled') fleet.ops = ops.value?.ops ?? []
    if (credentials.status === 'fulfilled') fleet.credentials = credentials.value ?? null
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
      ['fleet-sync-changed', (payload) => { if (payload?.status?.state) fleet.status = payload.status }],
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

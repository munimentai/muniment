// The messages the user sent during each reply, by run id. The runtime
// takes such a message into the running reply and keeps no record of where,
// so the window keeps it to show the message inside the reply after a reload.
export const RUN_STEERS_KEY = 'muniment.run-steers'
// The newest replies that keep their messages.
const KEPT_RUNS = 200

export function readRunSteers(storage = globalThis.localStorage) {
  try {
    const value = JSON.parse(storage?.getItem(RUN_STEERS_KEY) || '{}')
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {}
    return Object.fromEntries(Object.entries(value)
      .map(([runId, steers]) => [runId, Array.isArray(steers) ? steers.filter((steer) => typeof steer?.text === 'string' && Number.isInteger(steer.textOffset) && steer.textOffset >= 0) : []])
      .filter(([, steers]) => steers.length))
  } catch (_) {
    return {}
  }
}

export function rememberRunSteer(runId, steer, storage = globalThis.localStorage) {
  if (!runId || typeof steer?.text !== 'string') return
  const steers = readRunSteers(storage)
  const next = [...(steers[runId] ?? []), { text: steer.text, textOffset: steer.textOffset }]
  delete steers[runId]
  steers[runId] = next
  const kept = Object.entries(steers).slice(-KEPT_RUNS)
  try {
    storage?.setItem(RUN_STEERS_KEY, JSON.stringify(Object.fromEntries(kept)))
  } catch (_) {
    // A full or blocked store keeps the message on screen until a reload.
  }
}

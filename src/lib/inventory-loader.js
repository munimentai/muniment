import { currentModel } from './provider-catalog.js'

// How long the shell waits before it reads the model inventory again. Right
// after a launch or a reply, a connected provider can answer before its
// models are listed, and the chip would offer to connect a model.
export const INVENTORY_RETRY_DELAYS = [1500, 3000, 6000]

// Whether an inventory names connected providers but no model the picker
// can show yet.
export function awaitingModels(inventory) {
  return (inventory?.providers?.length ?? 0) > 0 && !currentModel(inventory)
}

// Reads the inventory, and reads it again after each delay while `awaiting`
// says the answer still lacks models. `onanswer(inventory, final)` gets each
// answer. `alive()` stops the reads when a newer request or the window takes over.
export async function loadInventory(invoke, onanswer, { delays = INVENTORY_RETRY_DELAYS, alive = () => true, wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms)), awaiting = awaitingModels } = {}) {
  for (let attempt = 0; ; attempt += 1) {
    const next = await invoke('local_mode_provider_inventory')
    if (!alive()) return
    const final = !awaiting(next) || attempt >= delays.length
    onanswer(next, final)
    if (final) return
    await wait(delays[attempt])
    if (!alive()) return
  }
}

// Whether a read of the machine whose models the chip names still lacks a
// model: a provider lists none yet, or the read lost the model the chip named.
export function awaitingFrom(previous) {
  return (next) => awaitingModels(next) || (!!currentModel(previous) && !currentModel(next))
}

// The inventory the shell keeps from an answer. A read that names no model
// keeps the model the chip named while reads continue, and after the last
// read too when `keep` says the read only refreshed the same machine, such as
// after a reply or a reconnect. Settings and a machine switch take the answer.
export function keptInventory(previous, next, final, { keep = false } = {}) {
  if (!currentModel(previous) || currentModel(next)) return next
  return final && !keep ? next : previous
}

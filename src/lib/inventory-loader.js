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

// Reads the inventory, and reads it again after each delay while a connected
// provider lists no model yet. `onanswer(inventory, final)` gets each answer.
// `alive()` stops the reads when a newer request or the window takes over.
export async function loadInventory(invoke, onanswer, { delays = INVENTORY_RETRY_DELAYS, alive = () => true, wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms)) } = {}) {
  for (let attempt = 0; ; attempt += 1) {
    const next = await invoke('local_mode_provider_inventory')
    if (!alive()) return
    const final = !awaitingModels(next) || attempt >= delays.length
    onanswer(next, final)
    if (final) return
    await wait(delays[attempt])
    if (!alive()) return
  }
}

// The inventory the shell keeps from an answer: a fresh one with models, the
// last answer, or the earlier inventory while a provider still lists none.
export function keptInventory(previous, next, final) {
  if (final || !awaitingModels(next) || !currentModel(previous)) return next
  return previous
}

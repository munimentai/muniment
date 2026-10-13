// Recall settings live in the private profile, where each new chat reads them.
export const RECALL_DEFAULTS = Object.freeze({ learn: true, use: true, ask: false })
const LEGACY_KEY = 'muniment.memory-recall'

export async function loadRecall(tauri) {
  const stored = await tauri.invoke('memory_recall_settings')
  return { ...RECALL_DEFAULTS, ...(stored || {}) }
}

export function saveRecall(tauri, settings) {
  return tauri.invoke('memory_recall_settings_save', { settings })
}

// Moves recall choices kept in this device's storage into the profile once.
// Asking before saving starts off whatever the device kept.
export async function migrateRecall(tauri) {
  let text
  try { text = localStorage.getItem(LEGACY_KEY) } catch (_) { return }
  if (text === null || !tauri) return
  let stored = null
  try { stored = JSON.parse(text) } catch (_) {}
  const kept = (key) => typeof stored?.[key] === 'boolean' ? stored[key] : RECALL_DEFAULTS[key]
  try {
    if (await tauri.invoke('memory_recall_settings') == null) {
      await saveRecall(tauri, { learn: kept('learn'), use: kept('use'), ask: false })
    }
    localStorage.removeItem(LEGACY_KEY)
  } catch (_) {}
}

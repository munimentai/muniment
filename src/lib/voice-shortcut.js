import { holdToTalkShortcut, validHoldToTalkShortcut } from './dictation-state.js'

export const VOICE_SHORTCUT_STORAGE_KEY = 'muniment.voice-shortcut'

export function createVoiceShortcutManager({ register, unregister, storage, onShortcut, onState }) {
  const registeredShortcuts = new Set()
  let task = Promise.resolve()
  let destroyed = false
  let started = false
  let startupSettled = false
  let pendingChanges = 0
  let queuedShortcut = holdToTalkShortcut()
  let state = {
    shortcut: holdToTalkShortcut(),
    changing: true,
    registered: false,
    error: false,
  }

  function update(next) {
    state = { ...state, ...next }
    if (!destroyed) onState(state)
  }

  async function unregisterShortcut(shortcut) {
    await unregister(shortcut)
    registeredShortcuts.delete(shortcut)
  }

  function restoreSavedShortcut(saved) {
    if (saved === null) storage.removeItem(VOICE_SHORTCUT_STORAGE_KEY)
    else storage.setItem(VOICE_SHORTCUT_STORAGE_KEY, saved)
  }

  async function registerInitialShortcut() {
    const fallback = holdToTalkShortcut()
    let saved
    try { saved = storage.getItem(VOICE_SHORTCUT_STORAGE_KEY) } catch (_) {}
    const preferred = validHoldToTalkShortcut(saved) ? saved : fallback
    try {
      await register(preferred, onShortcut)
      registeredShortcuts.add(preferred)
      if (destroyed) return
      update({ shortcut: preferred, registered: true })
    } catch (_) {
      if (preferred !== fallback) {
        try {
          await register(fallback, onShortcut)
          registeredShortcuts.add(fallback)
          if (destroyed) return
          update({ shortcut: fallback, registered: true })
          return
        } catch (_) {}
      }
      update({ error: true })
    }
  }

  async function applyChange(next) {
    update({ error: false })
    const previous = state.shortcut
    const previousRegistered = state.registered
    let previousSaved
    try { previousSaved = storage.getItem(VOICE_SHORTCUT_STORAGE_KEY) } catch (_) {
      update({ error: true })
      return false
    }
    let nextRegistered = false
    let savedChanged = false
    let rollbackFailed = false
    try {
      await register(next, onShortcut)
      registeredShortcuts.add(next)
      nextRegistered = true
      if (destroyed) throw new Error('destroyed')
      storage.setItem(VOICE_SHORTCUT_STORAGE_KEY, next)
      savedChanged = true
      if (destroyed) throw new Error('destroyed')
      if (previousRegistered) await unregisterShortcut(previous)
      if (destroyed) throw new Error('destroyed')
      update({ shortcut: next, registered: true })
      return true
    } catch (_) {
      if (savedChanged) {
        try { restoreSavedShortcut(previousSaved) } catch (_) { rollbackFailed = true }
      }
      if (nextRegistered) {
        try { await unregisterShortcut(next) } catch (_) { rollbackFailed = true }
      }
      if (previousRegistered && !registeredShortcuts.has(previous)) {
        try {
          await register(previous, onShortcut)
          registeredShortcuts.add(previous)
        } catch (_) { rollbackFailed = true }
      }
      update({
        shortcut: previous,
        registered: registeredShortcuts.has(previous),
        error: true,
      })
      return rollbackFailed ? null : false
    }
  }

  function start() {
    started = true
    task = registerInitialShortcut().finally(async () => {
      startupSettled = true
      queuedShortcut = state.shortcut
      update({ changing: pendingChanges > 0 })
      if (destroyed) await cleanupRegisteredShortcuts()
    })
    return task
  }

  function change(next) {
    if (!started || !startupSettled || destroyed || next === queuedShortcut || !validHoldToTalkShortcut(next)) return next === queuedShortcut
    pendingChanges += 1
    queuedShortcut = next
    update({ changing: true })
    task = task.then(() => applyChange(next)).finally(() => {
      pendingChanges -= 1
      if (pendingChanges === 0) queuedShortcut = state.shortcut
      update({ changing: pendingChanges > 0 })
    })
    return task
  }

  async function cleanupRegisteredShortcuts() {
    state = { ...state, registered: false }
    await Promise.allSettled([...registeredShortcuts].map((shortcut) => unregisterShortcut(shortcut)))
  }

  function cleanup() {
    destroyed = true
    task = task.finally(cleanupRegisteredShortcuts)
    return task
  }

  return { start, change, cleanup }
}

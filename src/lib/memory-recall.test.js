import { afterEach, expect, it, vi } from 'vitest'

import { RECALL_DEFAULTS, loadRecall, migrateRecall, saveRecall } from './memory-recall.js'

afterEach(() => localStorage.clear())

const native = (stored = null) => {
  const state = { stored }
  const invoke = vi.fn(async (command, payload) => {
    if (command === 'memory_recall_settings') return state.stored
    if (command === 'memory_recall_settings_save') { state.stored = payload.settings; return payload.settings }
    throw new Error(`unexpected command: ${command}`)
  })
  return { invoke, state }
}

it('loads the profile file over the defaults and saves the whole choice', async () => {
  expect(RECALL_DEFAULTS).toEqual({ learn: true, use: true, ask: false })
  const tauri = native()
  expect(await loadRecall(tauri)).toEqual({ learn: true, use: true, ask: false })
  tauri.state.stored = { learn: false, use: true, ask: true }
  expect(await loadRecall(tauri)).toEqual({ learn: false, use: true, ask: true })
  expect(await saveRecall(tauri, { learn: true, use: false, ask: false })).toEqual({ learn: true, use: false, ask: false })
  expect(tauri.invoke).toHaveBeenLastCalledWith('memory_recall_settings_save', { settings: { learn: true, use: false, ask: false } })
})

it('does nothing without a device choice', async () => {
  const tauri = native()
  await migrateRecall(tauri)
  expect(tauri.invoke).not.toHaveBeenCalled()
})

it('moves the device choice into a missing profile file with ask off, then forgets it', async () => {
  localStorage.setItem('muniment.memory-recall', JSON.stringify({ learn: false, use: true, ask: true }))
  const tauri = native()
  await migrateRecall(tauri)
  expect(tauri.state.stored).toEqual({ learn: false, use: true, ask: false })
  expect(localStorage.getItem('muniment.memory-recall')).toBeNull()
})

it('keeps an existing profile file and forgets the device choice', async () => {
  localStorage.setItem('muniment.memory-recall', JSON.stringify({ learn: false, use: false }))
  const tauri = native({ learn: true, use: true, ask: true })
  await migrateRecall(tauri)
  expect(tauri.invoke).not.toHaveBeenCalledWith('memory_recall_settings_save', expect.anything())
  expect(tauri.state.stored).toEqual({ learn: true, use: true, ask: true })
  expect(localStorage.getItem('muniment.memory-recall')).toBeNull()
})

it('uses the defaults for a malformed device choice', async () => {
  localStorage.setItem('muniment.memory-recall', '{')
  const tauri = native()
  await migrateRecall(tauri)
  expect(tauri.state.stored).toEqual({ learn: true, use: true, ask: false })
  expect(localStorage.getItem('muniment.memory-recall')).toBeNull()
})

it('keeps the device choice when the profile cannot save it', async () => {
  localStorage.setItem('muniment.memory-recall', JSON.stringify({ learn: false }))
  const tauri = { invoke: vi.fn(async (command) => { if (command === 'memory_recall_settings') return null; throw 'Memory settings cannot be saved.' }) }
  await migrateRecall(tauri)
  expect(localStorage.getItem('muniment.memory-recall')).not.toBeNull()
})

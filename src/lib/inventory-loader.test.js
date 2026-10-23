import { describe, expect, it, vi } from 'vitest'
import { awaitingFrom, awaitingModels, keptInventory, loadInventory } from './inventory-loader.js'

const router = { id: 'muniment-router', name: 'Router', source: 'router', models: [] }
const listing = (models) => ({ providers: [router], hidden: [], shown: [], router_models: models.map((model) => ({ id: `openai/${model}`, family: 'openai', model, accounts: 1 })) })

describe('model inventory reads', () => {
  it('reads again while a connected provider lists no model yet', async () => {
    const empty = listing([])
    const ready = listing(['gpt-6-astra'])
    const invoke = vi.fn().mockResolvedValueOnce(empty).mockResolvedValueOnce(ready)
    const answers = []
    await loadInventory(invoke, (next, final) => answers.push([next, final]), { delays: [5, 5], wait: async () => {} })
    expect(answers).toEqual([[empty, false], [ready, true]])
  })

  it('reads once when nothing is connected or a model is listed', async () => {
    const invoke = vi.fn().mockResolvedValue({ providers: [], hidden: [] })
    await loadInventory(invoke, () => {}, { wait: async () => {} })
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(awaitingModels({ providers: [], hidden: [] })).toBe(false)
    expect(awaitingModels(listing([]))).toBe(true)
    expect(awaitingModels(listing(['gpt-6-astra']))).toBe(false)
  })

  it('keeps the chip on its model while a restarted runtime lists none, until the last read', () => {
    const before = listing(['gpt-6-astra'])
    const empty = listing([])
    expect(keptInventory(before, empty, false)).toBe(before)
    expect(keptInventory(before, empty, true)).toBe(empty)
    expect(keptInventory(null, empty, false)).toBe(empty)
  })

  it('keeps the model after the last read of the same machine and reads again while the model is gone', () => {
    const before = listing(['gpt-6-astra'])
    const gone = { providers: [], hidden: [], shown: [], router_models: [] }
    expect(keptInventory(before, gone, false)).toBe(before)
    expect(keptInventory(before, gone, true)).toBe(gone)
    expect(keptInventory(before, gone, true, { keep: true })).toBe(before)
    expect(keptInventory(before, listing(['gpt-6-luna']), true, { keep: true }).router_models[0].model).toBe('gpt-6-luna')
    expect(awaitingFrom(before)(gone)).toBe(true)
    expect(awaitingFrom(null)(gone)).toBe(false)
    expect(awaitingFrom(before)(before)).toBe(false)
  })

  it('stops reading when a newer request takes over', async () => {
    const invoke = vi.fn().mockResolvedValue(listing([]))
    let live = true
    await loadInventory(invoke, () => { live = false }, { delays: [1, 1], alive: () => live, wait: async () => {} })
    expect(invoke).toHaveBeenCalledTimes(1)
  })
})

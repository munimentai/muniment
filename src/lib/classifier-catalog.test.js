import { describe, expect, it } from 'vitest'
import { DEDICATED, SELF_HOSTED, POOLED, catalog, matchSaved, pooledReady, priceLabel } from './classifier-catalog.js'

describe('classifier catalog', () => {
  it('lists a model built to classify before the models on your own accounts', () => {
    const rows = catalog([])
    expect(rows[0].id).toBe('typesafe/jev-latest')
    expect(rows[0].group).toBe('Built to classify')
    expect(rows.slice(DEDICATED.length + SELF_HOSTED.length).every((row) => row.group === 'On your accounts')).toBe(true)
    expect(rows.length).toBe(DEDICATED.length + SELF_HOSTED.length + POOLED.length)
  })

  it('offers a pooled classifier only while its provider holds an enabled account', () => {
    const accounts = [{ family: 'openai', enabled: true }, { family: 'xai', enabled: false }]
    expect(pooledReady(POOLED[0], accounts)).toBe(true)
    expect(pooledReady(POOLED.find((entry) => entry.family === 'xai'), accounts)).toBe(false)
    expect(pooledReady(POOLED[0], [])).toBe(false)
    const rows = catalog(accounts)
    expect(rows.find((row) => row.id === 'openai/gpt-5.6-luna').ready).toBe(true)
    expect(rows.find((row) => row.id === 'kimi/kimi-k3').ready).toBe(false)
    expect(rows[0].ready).toBe(true)
  })

  it('reads a price per million input tokens and calls a free one free', () => {
    expect(priceLabel(0.042)).toBe('$0.042/M in')
    expect(priceLabel(0.2)).toBe('$0.2/M in')
    expect(priceLabel(1)).toBe('$1/M in')
    expect(priceLabel(0)).toBe('free')
    expect(priceLabel(undefined)).toBe('Price unavailable')
  })

  it('marks the row a saved classifier came from', () => {
    expect(matchSaved({ kind: 'typesafe' })).toBe('typesafe/jev-latest')
    expect(matchSaved({ kind: 'pooled', family: 'openai', model: 'gpt-5.6-luna' })).toBe('openai/gpt-5.6-luna')
    expect(matchSaved({ kind: 'pooled', family: 'openai', model: 'gpt-5.6' })).toBe('openai/gpt-5.6')
    expect(matchSaved({ kind: 'endpoint' })).toBe('endpoint')
    expect(matchSaved({ kind: 'endpoint', model: 'decider-2b' })).toBe('mapika/decider-2b')
    expect(matchSaved({ kind: 'none' })).toBe('')
    expect(matchSaved(null)).toBe('')
  })
})

it('includes discovered routing models without duplicates or invented prices', () => {
  const rows = catalog([{ family: 'xai', enabled: true, servable: true }], [{ family: 'xai', model: 'grok-4.7', name: 'Grok 4.7' }, { family: 'xai', model: 'grok-4.6' }])
  expect(rows.filter(row => row.model === 'grok-4.6')).toHaveLength(1)
  const fresh = rows.find(row => row.model === 'grok-4.7')
  expect(fresh.ready).toBe(true)
  expect(priceLabel(fresh.price)).toBe('Price unavailable')
})

import { describe, expect, it } from 'vitest'
import { DEDICATED, SELF_HOSTED, catalog, matchSaved, priceLabel } from './classifier-catalog.js'

describe('classifier catalog', () => {
  it('suggests only dedicated classifiers and compatible endpoints', () => {
    const rows = catalog([{ family: 'openai', enabled: true }], [{ family: 'openai', model: 'gpt-6-sol' }, { family: 'anthropic', model: 'claude-opus-5-5' }])
    expect(rows[0].id).toBe('typesafe/jev-latest')
    expect(rows.length).toBe(DEDICATED.length + SELF_HOSTED.length)
    expect(rows.some(row => row.kind === 'pooled' || row.group === 'On your accounts')).toBe(false)
    expect(rows.map(row => row.model)).not.toContain('gpt-6-sol')
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

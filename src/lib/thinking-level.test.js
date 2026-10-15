import { describe, expect, it } from 'vitest'
import { modelThinking, thinkingLabel, withThinkingLevel } from './thinking-level.js'

const inventory = {
  providers: [
    { id: 'openai', models: [
      { id: 'gpt', thinking_levels: ['low', 'medium', 'high', 'xhigh'], thinking_default: 'xhigh' },
      { id: 'mini', thinking_levels: ['off', 'low'], thinking_default: 'max' },
    ] },
    { id: 'ollama', models: [{ id: 'llama3.2:3b', thinking: false }, { id: 'qwen3:8b', thinking_levels: [] }] },
    { id: 'muniment-router', models: [{ id: 'openai/gpt', thinking_levels: ['off', 'high'], thinking_default: 'high' }] },
  ],
}

describe('thinking level', () => {
  it('reads the chip model levels and the level a send carries', () => {
    expect(modelThinking(inventory, { provider: 'openai', model: 'gpt' })).toEqual({ levels: ['low', 'medium', 'high', 'xhigh'], level: 'xhigh' })
    expect(modelThinking(inventory, { provider: 'muniment-router', model: 'openai/gpt' })).toEqual({ levels: ['off', 'high'], level: 'high' })
    // A default the levels do not hold falls back to the first level.
    expect(modelThinking(inventory, { provider: 'openai', model: 'mini' }).level).toBe('off')
  })

  it('offers nothing for a model without reasoning, an unknown model or no model', () => {
    expect(modelThinking(inventory, { provider: 'ollama', model: 'llama3.2:3b' })).toBeNull()
    expect(modelThinking(inventory, { provider: 'ollama', model: 'qwen3:8b' })).toBeNull()
    expect(modelThinking(inventory, { provider: 'openai', model: 'missing' })).toBeNull()
    expect(modelThinking(inventory, null)).toBeNull()
    expect(modelThinking(null, { provider: 'openai', model: 'gpt' })).toBeNull()
  })

  it('sets one model level without changing the inventory it read', () => {
    const next = withThinkingLevel(inventory, 'openai', 'gpt', 'low')
    expect(modelThinking(next, { provider: 'openai', model: 'gpt' }).level).toBe('low')
    expect(modelThinking(inventory, { provider: 'openai', model: 'gpt' }).level).toBe('xhigh')
    expect(next.providers[1]).toBe(inventory.providers[1])
    expect(next.providers[0].models[1]).toBe(inventory.providers[0].models[1])
  })

  it('names each level in sentence case', () => {
    expect(['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'].map(thinkingLabel))
      .toEqual(['Off', 'Minimal', 'Low', 'Medium', 'High', 'Extra high', 'Max'])
  })
})

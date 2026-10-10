import { expect, it } from 'vitest'
import { leadingModels, newestFirst } from './model-recency.js'

it('leads with the newest model of each family the provider serves', () => {
  const lead = leadingModels('anthropic', ['claude-opus-5-5', 'claude-opus-4-8', 'claude-sonnet-5-5', 'claude-sonnet-4-6'])
  expect([...lead]).toEqual(['claude-opus-5-5', 'claude-sonnet-5-5'])
  // Among the models served, an older generation still leads its family.
  expect([...leadingModels('anthropic', ['claude-opus-4-8', 'claude-opus-4-7'])]).toEqual(['claude-opus-4-8'])
})

it('leads with a model newer than the snapshot and every model of an unknown provider', () => {
  expect([...leadingModels('anthropic', ['claude-opus-5-5', 'claude-opus-9'])]).toEqual(['claude-opus-5-5', 'claude-opus-9'])
  expect([...leadingModels('ollama', ['llama3.2:3b', 'qwen3:8b'])]).toEqual(['llama3.2:3b', 'qwen3:8b'])
})

it('drops a family that has shipped nothing for a year behind the provider', () => {
  expect([...leadingModels('openai', ['gpt-6.1-sol', 'gpt-4o'])]).toEqual(['gpt-6.1-sol'])
})

it('orders models newest first, with a model newer than the snapshot at the top', () => {
  expect(newestFirst('openai', ['gpt-5.3-codex-spark', 'gpt-5.5', 'gpt-6.1-sol', 'gpt-7', 'gpt-6-astra'])).toEqual(['gpt-7', 'gpt-6.1-sol', 'gpt-6-astra', 'gpt-5.5', 'gpt-5.3-codex-spark'])
  expect(newestFirst('ollama', ['b', 'a'])).toEqual(['b', 'a'])
})

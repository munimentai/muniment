import { expect, it } from 'vitest'
import { newestFirst, pastModels } from './model-recency.js'

it('marks deprecated models and families a year behind the provider as past', () => {
  expect([...pastModels('openai', ['gpt-6.1-sol', 'gpt-4o', 'gpt-5.3-chat-latest', 'gpt-5.5'])]).toEqual(['gpt-4o', 'gpt-5.3-chat-latest'])
  expect([...pastModels('anthropic', ['claude-opus-5-5', 'claude-opus-4-8', 'claude-opus-9'])]).toEqual([])
  expect([...pastModels('ollama', ['llama3.2:3b', 'qwen3:8b'])]).toEqual([])
})

it('orders models newest first, with a model newer than the snapshot at the top', () => {
  expect(newestFirst('openai', ['gpt-5.3-codex-spark', 'gpt-5.5', 'gpt-6.1-sol', 'gpt-7', 'gpt-6-astra'])).toEqual(['gpt-7', 'gpt-6.1-sol', 'gpt-6-astra', 'gpt-5.5', 'gpt-5.3-codex-spark'])
  expect(newestFirst('ollama', ['b', 'a'])).toEqual(['b', 'a'])
})

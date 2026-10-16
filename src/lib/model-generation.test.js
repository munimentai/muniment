import { expect, it } from 'vitest'
import { currentGeneration, modelGeneration } from './model-generation.js'

it('reads the family and version from the id', () => {
  expect(modelGeneration('claude-opus-5-5')).toEqual({ family: 'claude-opus', version: [5, 5] })
  expect(modelGeneration('claude-opus-4-5-20251101')).toEqual({ family: 'claude-opus', version: [4, 5] })
  expect(modelGeneration('claude-3-5-sonnet-20241022')).toEqual({ family: 'claude-sonnet', version: [3, 5] })
  expect(modelGeneration('gpt-6.1-sol')).toEqual({ family: 'gpt-sol', version: [6, 1] })
  expect(modelGeneration('gpt-4o-mini')).toEqual({ family: 'gpt-o-mini', version: [4] })
  expect(modelGeneration('o3-pro')).toEqual({ family: 'o-pro', version: [3] })
  expect(modelGeneration('gemini-3.1-pro-preview')).toEqual({ family: 'gemini-pro', version: [3, 1] })
  expect(modelGeneration('gemini-2.5-computer-use-preview-10-2025')).toEqual({ family: 'gemini-computer-use', version: [2, 5] })
  expect(modelGeneration('openai/gpt-5.5')).toEqual({ family: 'gpt', version: [5, 5] })
})

it('leaves sizes and tags out of the family', () => {
  expect(modelGeneration('qwen3:8b')).toEqual({ family: 'qwen', version: [3] })
  expect(modelGeneration('qwen3.6-32k:latest')).toEqual({ family: 'qwen', version: [3, 6] })
  expect(modelGeneration('gemma-4-26b-a4b-it')).toEqual({ family: 'gemma-it', version: [4] })
  expect(modelGeneration('mistral-large-latest')).toEqual({ family: 'mistral-large', version: null })
})

it('keeps the newest generation of each family', () => {
  const ids = ['claude-opus-5-5', 'claude-opus-5', 'claude-opus-4-8', 'claude-sonnet-5-5', 'claude-sonnet-4-6', 'claude-haiku-4-5', 'claude-haiku-4-5-20251001']
  expect([...currentGeneration(ids)]).toEqual(['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-4-5', 'claude-haiku-4-5-20251001'])
  expect([...currentGeneration(['gpt-5.6', 'gpt-5', 'gpt-5.4-mini', 'gpt-4.1-mini', 'gpt-6-luna', 'gpt-5.6-luna'])]).toEqual(['gpt-5.6', 'gpt-5.4-mini', 'gpt-6-luna'])
  expect([...currentGeneration(['qwen3:8b', 'qwen3:32b', 'qwen2.5:7b', 'llama3.2:3b'])]).toEqual(['qwen3:8b', 'qwen3:32b', 'llama3.2:3b'])
})

it('reads a minor version as a decimal and counts an id without a version as current', () => {
  expect([...currentGeneration(['grok-4.20', 'grok-4.7', 'grok-4.3'])]).toEqual(['grok-4.7'])
  expect([...currentGeneration(['gpt-oss:20b', 'custom-model', 'gemini-flash-latest', 'gemini-3.8-flash', 'gemini-3.7-flash'])]).toEqual(['gpt-oss:20b', 'custom-model', 'gemini-flash-latest', 'gemini-3.8-flash'])
})

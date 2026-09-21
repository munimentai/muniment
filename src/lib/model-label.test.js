import { expect, it } from 'vitest'
import { modelLabel } from './model-label.js'
import { pickerGroups } from './provider-catalog.js'

it('formats provider names, versions, sizes, and model variants consistently', () => {
  for (const [id, name] of [
    ['openai/gpt-5.6-terra', 'OpenAI · GPT 5.6 Terra'],
    ['typesafe/jev-latest', 'TypeSafe · Jev Latest'],
    ['claude-haiku-4-5', 'Claude Haiku 4.5'],
    ['semif-qwen3.5-4b', 'SemIf Qwen 3.5 4B'],
    ['decider-2b', 'Decider 2B'],
    ['custom-model-2026-09-21', 'Custom Model 2026 09 21'],
  ]) expect(modelLabel(id)).toBe(name)
})

it('finds models by readable names or exact IDs and retains the connection ID', () => {
  const inventory = { providers: [{ id: 'openai', name: 'OpenAI', models: [{ id: 'gpt-5.6-terra' }] }] }
  for (const query of ['GPT 5.6 Terra', 'gpt-5.6-terra']) {
    expect(pickerGroups(inventory, query)[0].models[0]).toMatchObject({
      id: 'gpt-5.6-terra', choice: 'gpt-5.6-terra', label: 'GPT 5.6 Terra',
    })
  }
})

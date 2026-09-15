import { describe, expect, it } from 'vitest'
import { PROVIDERS, catalogProvider, connectableProviders, currentModel, methodLabel, modelChipLabel, pickerGroups, providerName, searchProviders, sourceTag } from './provider-catalog.js'

const inventory = {
  providers: [
    { id: 'openai-codex', name: 'OpenAI', source: 'account', base_url: null, models: [{ id: 'gpt-5.5', context: '400K', max_out: '128K', thinking: true, images: true }, { id: 'gpt-5.5-mini', context: '400K', max_out: '128K', thinking: true, images: true }] },
    { id: 'ollama', name: 'Ollama', source: 'local', base_url: 'http://localhost:11434/v1', models: [{ id: 'llama3.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false }] },
    { id: 'anthropic', name: 'Anthropic', source: 'key', base_url: null, models: [] },
  ],
  default_provider: 'ollama',
  default_model: 'llama3.2:3b',
  hidden: ['openai-codex/gpt-5.5-mini'],
}

describe('provider catalog', () => {
  it('puts the eight featured providers first and searches the rest by name or id', () => {
    const { popular, other } = searchProviders()
    expect(popular.map((provider) => provider.id)).toEqual(['anthropic', 'openai', 'xai', 'google', 'openrouter', 'ollama', 'lmstudio', 'custom'])
    expect(other.length).toBe(PROVIDERS.length - 8)
    expect(other.map((provider) => provider.name)).toEqual([...other.map((provider) => provider.name)].sort((a, b) => a.localeCompare(b)))
    expect(searchProviders('grok').popular).toEqual([])
    expect(searchProviders('xai').popular.map((provider) => provider.id)).toEqual(['xai'])
    expect(searchProviders('Fire').other.map((provider) => provider.id)).toEqual(['fireworks'])
  })

  it('offers an account where Pi signs in, Claude Code for Anthropic, and a key or a server URL elsewhere', () => {
    expect(catalogProvider('openai').methods).toEqual(['account', 'key'])
    expect(methodLabel(catalogProvider('openai'), 'account')).toBe('ChatGPT Plus or Pro account')
    expect(catalogProvider('openai').account.provider).toBe('openai-codex')
    expect(methodLabel(catalogProvider('xai'), 'account')).toBe('SuperGrok or X Premium account')
    expect(catalogProvider('anthropic').methods).toEqual(['claude-code', 'key'])
    expect(methodLabel(catalogProvider('anthropic'), 'claude-code')).toBe('Claude Code sign-in')
    expect(catalogProvider('google').methods).toEqual(['key'])
    expect(catalogProvider('lmstudio').methods).toEqual(['endpoint'])
    expect(catalogProvider('ollama').baseUrl).toBe('http://localhost:11434/v1')
    for (const provider of PROVIDERS) {
      expect(provider.methods.length).toBeGreaterThan(0)
      if (provider.methods.includes('account')) expect(provider.account.provider).toBeTruthy()
    }
  })

  it('names Pi ids by the provider the user connected and tags each source', () => {
    expect(providerName('openai-codex')).toBe('OpenAI')
    expect(providerName('claude-bridge')).toBe('Anthropic')
    expect(providerName('custom-proxy', { providers: [{ id: 'custom-proxy', name: 'My proxy' }] })).toBe('My proxy')
    expect(providerName('unknown-id')).toBe('unknown-id')
    expect(['key', 'account', 'local', 'custom', 'claude-code'].map(sourceTag)).toEqual(['Key', 'Account', 'Local', 'Custom', 'Claude Code'])
  })

  it('groups shown models by provider, drops hidden ones and empty providers, and searches ids', () => {
    expect(pickerGroups(inventory).map((group) => [group.id, group.models.map((model) => model.id)])).toEqual([
      ['openai-codex', ['gpt-5.5']],
      ['ollama', ['llama3.2:3b']],
    ])
    expect(pickerGroups(inventory, 'llama').map((group) => group.id)).toEqual(['ollama'])
    expect(pickerGroups(inventory, 'openai').map((group) => group.id)).toEqual(['openai-codex'])
    expect(pickerGroups(null)).toEqual([])
  })

  it('offers the popular providers that are not connected, counting aliases as connected', () => {
    expect(connectableProviders(inventory).map((provider) => provider.id)).toEqual(['xai', 'google', 'openrouter', 'lmstudio', 'custom'])
    const withEndpoint = { providers: [{ id: 'custom-litellm', name: 'LiteLLM', source: 'custom', models: [] }, { id: 'claude-bridge', name: 'Anthropic', source: 'claude-code', models: [] }] }
    expect(connectableProviders(withEndpoint).map((provider) => provider.id)).toEqual(['openai', 'xai', 'google', 'openrouter', 'ollama', 'lmstudio'])
    expect(connectableProviders(null)).toHaveLength(8)
  })

  it('reads the saved default when it is shown, else the first shown model, and labels the chip', () => {
    expect(currentModel(inventory)).toEqual({ provider: 'ollama', model: 'llama3.2:3b' })
    expect(modelChipLabel(inventory)).toBe('llama3.2:3b')
    const hiddenDefault = { ...inventory, hidden: ['ollama/llama3.2:3b'] }
    expect(currentModel(hiddenDefault)).toEqual({ provider: 'openai-codex', model: 'gpt-5.5' })
    expect(modelChipLabel({ providers: [], hidden: [] })).toBe('Connect a model')
    expect(modelChipLabel(null)).toBe('Connect a model')
  })
})

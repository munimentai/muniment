import { describe, expect, it } from 'vitest'
import { PROVIDERS, catalogProvider, connectableProviders, currentModel, methodLabel, modelChipLabel, pickerGroups, pickerLabel, providerName, searchProviders, sourceTag } from './provider-catalog.js'

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

describe('the model router in the picker', () => {
  const routed = {
    providers: [
      { id: 'openai-codex', name: 'OpenAI', source: 'account', base_url: null, models: [{ id: 'gpt-5.5', context: '400K' }] },
      { id: 'muniment-router', name: 'Model router', source: 'router', base_url: 'http://127.0.0.1:8421/v1', models: [{ id: 'auto', context: '' }, { id: 'fast', context: '' }] },
    ],
    default_provider: 'openai-codex',
    default_model: 'gpt-5.5',
    hidden: [],
    router_classifier: 'jev-latest',
  }

  it('leads the picker with the router while a classifier picks the model per turn', () => {
    const groups = pickerGroups(routed)
    expect(groups.map((group) => group.id)).toEqual(['muniment-router', 'openai-codex'])
    expect(groups[0].classifier).toBe('jev-latest')
    expect(groups[1].classifier).toBe('')
    expect(sourceTag('router')).toBe('Router')
  })

  it('names the classifier row by the model that picks and puts it first', () => {
    const groups = pickerGroups(routed)
    expect(groups[0].models[0]).toMatchObject({ id: 'auto', label: 'jev-latest picks' })
    expect(pickerLabel({ id: 'fast' }, groups[0])).toBe('fast')
    expect(modelChipLabel({ ...routed, default_provider: 'muniment-router', default_model: 'auto' })).toBe('jev-latest picks')
  })

  it('marks a router model that more than one account serves', () => {
    const pooled = { ...routed, router_accounts: { auto: 0, fast: 2 } }
    const router = pickerGroups(pooled).find((group) => group.id === 'muniment-router')
    expect(router.models.map((model) => [model.id, model.accounts])).toEqual([['auto', 0], ['fast', 2]])
    expect(pickerGroups(routed).find((group) => group.id === 'openai-codex').models.every((model) => model.accounts === 0)).toBe(true)
  })

  it('leaves the order alone and drops the auto row when the router runs no classifier', () => {
    const balanced = { ...routed, router_classifier: null }
    expect(pickerGroups(balanced).map((group) => group.id)).toEqual(['openai-codex', 'muniment-router'])
    expect(pickerGroups(balanced).find((group) => group.id === 'muniment-router').models.map((model) => model.id)).toEqual(['fast'])
    expect(pickerGroups(balanced)[1].classifier).toBe('')
  })

  it('keeps the saved default when the router leads the list', () => {
    expect(currentModel(routed)).toEqual({ provider: 'openai-codex', model: 'gpt-5.5' })
    expect(modelChipLabel(routed)).toBe('gpt-5.5')
    const unset = { ...routed, default_provider: null, default_model: null }
    expect(currentModel(unset)).toEqual({ provider: 'muniment-router', model: 'auto' })
  })
})

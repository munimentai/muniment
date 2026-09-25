import { describe, expect, it } from 'vitest'
import { PROVIDERS, catalogProvider, connectableProviders, currentModel, currentModelAvailable, methodLabel, modelChipLabel, pickerGroups, providerName, searchProviders, sourceTag } from './provider-catalog.js'

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
    expect(searchProviders('grok').popular.map((provider) => provider.id)).toEqual(['xai'])
    expect(searchProviders('xai').popular.map((provider) => provider.id)).toEqual(['xai'])
    expect(searchProviders('Fire').other.map((provider) => provider.id)).toEqual(['fireworks'])
  })

  it('offers an account where Pi signs in, Claude Code for Anthropic, and a key or a server URL elsewhere', () => {
    expect(catalogProvider('openai').methods).toEqual(['account', 'key'])
    expect(methodLabel(catalogProvider('openai'), 'account')).toBe('ChatGPT Plus or Pro account')
    expect(catalogProvider('openai').account.provider).toBe('openai-codex')
    expect(methodLabel(catalogProvider('xai'), 'account')).toBe('Grok Build account')
    expect(catalogProvider('anthropic').methods).toEqual(['claude-code', 'key'])
    expect(methodLabel(catalogProvider('anthropic'), 'claude-code')).toBe('Claude Code sign-in')
    expect(catalogProvider('google').methods).toEqual(['account', 'key'])
    expect(catalogProvider('lmstudio').methods).toEqual(['endpoint'])
    expect(catalogProvider('ollama').baseUrl).toBe('http://localhost:11434/v1')
    for (const provider of PROVIDERS) {
      expect(provider.methods.length).toBeGreaterThan(0)
      if (provider.methods.includes('account')) expect(provider.account.provider).toBeTruthy()
    }
  })

  it('offers all six subscription sign-ins and Muse Code through provider search', () => {
    const cases = [
      ['Antigravity', 'google', 'antigravity'],
      ['ChatGPT', 'openai', 'openai-codex'],
      ['Claude Code', 'anthropic', null],
      ['Grok', 'xai', 'xai'],
      ['Devin', 'devin', 'devin'],
      ['Kimi Code', 'kimi', 'kimi'],
      ['Muse Code', 'meta', 'meta'],
    ]
    for (const [query, id, signIn] of cases) {
      const results = searchProviders(query)
      expect([...results.popular, ...results.other].map((p) => p.id)).toContain(id)
      const provider = catalogProvider(id)
      expect(provider.methods).toContain(signIn ? 'account' : 'claude-code')
      if (signIn) expect(provider.account.provider).toBe(signIn)
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
    expect(currentModel(inventory)).toMatchObject({ provider: 'ollama', model: 'llama3.2:3b' })
    expect(modelChipLabel(inventory)).toBe('Llama 3.2:3B')
    const hiddenDefault = { ...inventory, hidden: ['ollama/llama3.2:3b'] }
    expect(currentModel(hiddenDefault)).toMatchObject({ provider: 'openai-codex', model: 'gpt-5.5' })
    expect(modelChipLabel({ providers: [], hidden: [] })).toBe('Connect a model')
    expect(modelChipLabel(null)).toBe('Connect a model')
  })
})

describe('the model router in the picker', () => {
  const routed = {
    providers: [
      { id: 'openai-codex', name: 'OpenAI', source: 'account', base_url: null, models: [{ id: 'gpt-5.5', context: '400K' }] },
      { id: 'muniment-router', name: 'Model router', source: 'router', base_url: 'http://127.0.0.1:8421/v1', models: [{ id: 'auto', context: '' }, { id: 'fast', context: '' }, { id: 'kimi/kimi-k3', context: '' }] },
    ],
    default_provider: 'openai-codex',
    default_model: 'gpt-5.5',
    hidden: [],
    router_classifier: 'jev-latest',
    router_models: [
      { id: 'fast', family: 'openai', model: 'gpt-5.5', accounts: 2 },
      { id: 'kimi/kimi-k3', family: 'kimi', model: 'kimi-k3', accounts: 1 },
    ],
  }

  it('leads the picker with the classifier row, named by the model that picks', () => {
    const groups = pickerGroups(routed)
    expect(groups.map((group) => group.id)).toEqual(['muniment-router', 'openai-codex', 'router:kimi'])
    expect(groups[0].classifier).toBe('jev-latest')
    expect(groups[0].models).toEqual([{ id: 'auto', context: '', label: 'Jev Latest picks', provider: 'muniment-router', choice: 'auto', accounts: 0 }])
    expect(groups[1].classifier).toBe('')
    expect(sourceTag('router')).toBe('Router')
    expect(modelChipLabel({ ...routed, default_provider: 'muniment-router', default_model: 'auto' })).toBe('Jev Latest picks')
  })

  it('lists a model once under its provider and routes it through the router when two accounts serve it', () => {
    const openai = pickerGroups(routed).find((group) => group.id === 'openai-codex')
    expect(openai.models).toEqual([{ id: 'gpt-5.5', context: '400K', label: 'GPT 5.5', provider: 'muniment-router', choice: 'fast', accounts: 2 }])
    // One pooled account still needs the native router transport.
    const single = { ...routed, router_models: [{ id: 'fast', family: 'openai', model: 'gpt-5.5', accounts: 1 }] }
    expect(pickerGroups(single).find((group) => group.id === 'openai-codex').models[0]).toMatchObject({ provider: 'muniment-router', choice: 'fast', accounts: 1 })
  })

  it('hides the pooled model without restoring its direct duplicate', () => {
    const groups = pickerGroups({ ...routed, hidden: ['muniment-router/fast'] })
    expect(groups.flatMap((group) => group.models).some((model) => model.id === 'gpt-5.5')).toBe(false)
  })

  it('gives a family with no provider of its own a group of its own', () => {
    const kimi = pickerGroups(routed).find((group) => group.id === 'router:kimi')
    expect(kimi.name).toBe('Kimi')
    expect(kimi.source).toBe('router')
    expect(kimi.models).toEqual([{ id: 'kimi-k3', context: '', label: 'Kimi K3', provider: 'muniment-router', choice: 'kimi/kimi-k3', accounts: 1 }])
    expect(pickerGroups({ ...routed, hidden: ['muniment-router/kimi/kimi-k3'] }).map((group) => group.id)).toEqual(['muniment-router', 'openai-codex'])
  })

  it('keeps automatic fallback selection when no classifier is configured', () => {
    const balanced = { ...routed, router_classifier: null }
    expect(pickerGroups(balanced).map((group) => group.id)).toEqual(['muniment-router', 'openai-codex', 'router:kimi'])
    expect(pickerGroups(balanced)[0].models[0].label).toBe('Automatic')
  })

  it('keeps the saved default when a row still saves it, and names the row in use', () => {
    expect(currentModel({ ...routed, default_provider: 'muniment-router', default_model: 'fast' })).toEqual({ provider: 'muniment-router', model: 'fast', label: 'GPT 5.5' })
    // Preserve a saved specific model when its pool becomes available.
    expect(currentModel(routed)).toEqual({ provider: 'muniment-router', model: 'fast', label: 'GPT 5.5' })
    expect(modelChipLabel(routed)).toBe('GPT 5.5')
    expect(currentModel({ ...routed, router_classifier: null, default_provider: 'openai', default_model: 'gone' })).toEqual({ provider: 'muniment-router', model: 'auto', label: 'Automatic' })
  })
})

it('deduplicates repeated model rows returned by discovery', () => {
  const inventory = { providers: [{ id: 'openai', name: 'OpenAI', source: 'key', models: [{ id: 'gpt' }, { id: 'gpt' }] }], hidden: [] }
  expect(pickerGroups(inventory)[0].models).toHaveLength(1)
})

it('groups several pooled models under one provider without a direct connection', () => {
  const inventory = { providers: [{ id: 'muniment-router', name: 'Router', source: 'router', models: [] }], hidden: [], router_models: [{ id: 'openai/a', family: 'openai', model: 'a', accounts: 2 }, { id: 'openai/b', family: 'openai', model: 'b', accounts: 2 }] }
  const groups = pickerGroups(inventory)
  expect(groups.map((group) => group.id)).toEqual(['muniment-router', 'router:openai'])
  expect(groups[1].models.map((model) => model.id)).toEqual(['a', 'b'])
})

describe('current model availability', () => {
  const routed = {
    providers: [{ id: 'muniment-router', name: 'Router', source: 'router', models: [] }],
    default_provider: 'muniment-router', default_model: 'auto', router_classifier: 'classifier',
    router_models: [{ id: 'openai/model-a', family: 'openai', model: 'model-a', accounts: 0 }],
  }
  it('does not mistake the automatic picker for an enabled account', () => {
    expect(currentModel(routed)?.model).toBe('auto')
    expect(currentModelAvailable(routed)).toBe(false)
    expect(currentModelAvailable({ ...routed, router_models: [{ ...routed.router_models[0], accounts: 1 }] })).toBe(true)
  })
  it('requires an account for the selected model, even when another model has one', () => {
    const mixed = { ...routed, default_model: 'openai/model-a', router_models: [...routed.router_models, { id: 'xai/model-b', family: 'xai', model: 'model-b', accounts: 1 }] }
    expect(currentModelAvailable(mixed)).toBe(false)
    expect(currentModelAvailable({ ...mixed, default_model: 'xai/model-b' })).toBe(true)
  })
  it('accepts a configured direct provider and rejects an empty inventory', () => {
    expect(currentModelAvailable(inventory)).toBe(true)
    expect(currentModelAvailable(null)).toBe(false)
    expect(currentModelAvailable({ providers: [] })).toBe(false)
  })
})

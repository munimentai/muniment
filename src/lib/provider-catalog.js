// The provider catalog behind Settings → Models: Pi's built-in providers, the
// method each one offers, and the helpers the section and the picker share.
// `account` names the Pi provider that signs in with an account and the label
// the method shows. Anthropic's account is the Claude Code sign-in through the
// pi-claude-bridge package, so it carries no Pi OAuth provider here.

export const PROVIDERS = [
  { id: 'anthropic', name: 'Anthropic', popular: true, methods: ['claude-code', 'key'] },
  { id: 'openai', name: 'OpenAI', popular: true, methods: ['account', 'key'], account: { provider: 'openai-codex', label: 'ChatGPT Plus or Pro account' } },
  { id: 'xai', name: 'xAI', popular: true, methods: ['account', 'key'], account: { provider: 'xai', label: 'SuperGrok or X Premium account' } },
  { id: 'google', name: 'Google', popular: true, methods: ['key'] },
  { id: 'openrouter', name: 'OpenRouter', popular: true, methods: ['account', 'key'], account: { provider: 'openrouter', label: 'OpenRouter account' } },
  { id: 'ollama', name: 'Ollama', popular: true, methods: ['ollama'], baseUrl: 'http://localhost:11434/v1' },
  { id: 'lmstudio', name: 'LM Studio', popular: true, methods: ['endpoint'], baseUrl: 'http://localhost:1234/v1' },
  { id: 'custom', name: 'Custom OpenAI-compatible endpoint', popular: true, methods: ['endpoint'] },
  { id: 'github-copilot', name: 'GitHub Copilot', methods: ['account'], account: { provider: 'github-copilot', label: 'GitHub Copilot account' } },
  { id: 'deepseek', name: 'DeepSeek', methods: ['key'] },
  { id: 'mistral', name: 'Mistral', methods: ['key'] },
  { id: 'groq', name: 'Groq', methods: ['key'] },
  { id: 'cerebras', name: 'Cerebras', methods: ['key'] },
  { id: 'nvidia', name: 'NVIDIA NIM', methods: ['key'] },
  { id: 'amazon-bedrock', name: 'Amazon Bedrock', methods: ['key'] },
  { id: 'azure-openai-responses', name: 'Azure OpenAI', methods: ['key'] },
  { id: 'zai', name: 'ZAI Coding Plan', methods: ['key'] },
  { id: 'opencode', name: 'OpenCode Zen', methods: ['key'] },
  { id: 'opencode-go', name: 'OpenCode Go', methods: ['key'] },
  { id: 'huggingface', name: 'Hugging Face', methods: ['key'] },
  { id: 'fireworks', name: 'Fireworks', methods: ['key'] },
  { id: 'together', name: 'Together', methods: ['key'] },
  { id: 'baseten', name: 'Baseten', methods: ['key'] },
  { id: 'kimi-coding', name: 'Kimi For Coding', methods: ['key'] },
  { id: 'minimax', name: 'MiniMax', methods: ['key'] },
  { id: 'qwen-token-plan', name: 'Qwen Token Plan', methods: ['key'] },
  { id: 'radius', name: 'Radius', methods: ['key'] },
]

const METHOD_LABELS = {
  'claude-code': 'Claude Code sign-in',
  account: 'Account',
  key: 'API key',
  ollama: 'Ollama server',
  endpoint: 'Server URL',
}

// Pi ids that reach the shell under another provider's name: the Codex account
// is OpenAI, the bridge is Anthropic.
const PROVIDER_ALIASES = { 'openai-codex': 'openai', 'claude-bridge': 'anthropic' }

export function catalogProvider(id) {
  return PROVIDERS.find((provider) => provider.id === id) ?? null
}

export function providerName(id, inventory = null) {
  const connected = inventory?.providers?.find((provider) => provider.id === id)
  if (connected?.name) return connected.name
  return catalogProvider(PROVIDER_ALIASES[id] ?? id)?.name ?? id
}

export function methodLabel(provider, method) {
  if (method === 'account') return provider.account?.label ?? METHOD_LABELS.account
  return METHOD_LABELS[method] ?? method
}

// The catalog split the connector shows: the popular eight first, then the rest
// by name. A query narrows both by name or id.
export function searchProviders(query = '') {
  const needle = query.trim().toLowerCase()
  const matches = PROVIDERS.filter((provider) => !needle
    || provider.name.toLowerCase().includes(needle)
    || provider.id.includes(needle))
  return {
    popular: matches.filter((provider) => provider.popular),
    other: matches.filter((provider) => !provider.popular).sort((a, b) => a.name.localeCompare(b.name)),
  }
}

// The catalog id a connected Pi provider counts as: the Codex account is
// OpenAI, the bridge is Anthropic, a named endpoint is the custom endpoint.
function connectedCatalogId(id) {
  if (id.startsWith('custom-')) return 'custom'
  return PROVIDER_ALIASES[id] ?? id
}

// The popular providers the Models list offers inline, in catalog order,
// without the ones already connected.
export function connectableProviders(inventory) {
  const connected = new Set((inventory?.providers ?? []).map((provider) => connectedCatalogId(provider.id)))
  return PROVIDERS.filter((provider) => provider.popular && !connected.has(provider.id))
}

export const SOURCE_TAGS = { key: 'Key', account: 'Account', local: 'Local', custom: 'Custom', 'claude-code': 'Claude Code', router: 'Router' }

export function sourceTag(source) {
  return SOURCE_TAGS[source] ?? source
}

export function modelKey(provider, model) {
  return `${provider}/${model}`
}

// The picker's rows: every model of every connected provider that is not hidden,
// grouped by provider in inventory order, narrowed by a query over the model id.
// A router running a classifier picks the model per turn, so its group leads.
export function pickerGroups(inventory, query = '') {
  if (!inventory || !Array.isArray(inventory.providers)) return []
  const hidden = new Set(inventory.hidden ?? [])
  const needle = query.trim().toLowerCase()
  const groups = inventory.providers
    .map((provider) => ({
      id: provider.id,
      name: provider.name,
      source: provider.source,
      classifier: provider.source === 'router' ? (inventory.router_classifier ?? '') : '',
      models: (provider.models ?? []).filter((model) => !hidden.has(modelKey(provider.id, model.id))
        && (!needle || model.id.toLowerCase().includes(needle) || provider.name.toLowerCase().includes(needle))),
    }))
    .filter((group) => group.models.length > 0)
  if (!inventory.router_classifier) return groups
  return [...groups.filter((group) => group.classifier), ...groups.filter((group) => !group.classifier)]
}

// The default Pi will use: the saved default when it is still shown, else the
// first shown model, else nothing.
export function currentModel(inventory) {
  if (!inventory) return null
  const groups = pickerGroups(inventory)
  const saved = inventory.default_provider && inventory.default_model
    ? groups.find((group) => group.id === inventory.default_provider)?.models.find((model) => model.id === inventory.default_model)
    : null
  if (saved) return { provider: inventory.default_provider, model: saved.id }
  const first = groups[0]
  return first ? { provider: first.id, model: first.models[0].id } : null
}

// The composer chip: the model id in use beside the provider's mark, `Connect a
// model` when none answers.
export function modelChipLabel(inventory) {
  const current = currentModel(inventory)
  if (!current) return 'Connect a model'
  return current.model
}

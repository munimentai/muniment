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

// The router's families under the catalog provider each one is, and the name
// a family group takes when no provider of its own is connected.
const FAMILY_CATALOG = { openai: 'openai', anthropic: 'anthropic', google: 'google', xai: 'xai', kimi: 'kimi-coding', devin: 'devin' }
const FAMILY_NAMES = { openai: 'OpenAI', anthropic: 'Anthropic', google: 'Google', xai: 'xAI', kimi: 'Kimi', devin: 'Devin' }

// The router family a connected provider pools into, none for a provider the
// router does not pool, and the catalog provider a family shows as.
export function providerFamily(id) {
  const catalogId = connectedCatalogId(id)
  return Object.entries(FAMILY_CATALOG).find(([, candidate]) => candidate === catalogId)?.[0] ?? null
}

export function familyProvider(family) {
  return FAMILY_CATALOG[family] ?? family
}

export function familyName(family) {
  return FAMILY_NAMES[family] ?? family
}

// The picker's rows: every model of every connected provider that is not
// hidden, grouped by provider in inventory order, narrowed by a query over the
// model id. Each row names the provider and the choice a pick saves, so a row
// the router serves saves the router's own id.
//
// The router's models sit under their provider, each model once: a model the
// provider also serves directly is one row, and it takes the router's route
// when two or more accounts stand behind it, so picking it balances across
// them with no other step. A family with no provider of its own becomes a
// group of its own. A running classifier is the first row of the picker,
// named by the model that picks, and choosing it hands the turn to
// classification, which is the router's auto.
export function pickerGroups(inventory, query = '') {
  if (!inventory || !Array.isArray(inventory.providers)) return []
  const hidden = new Set(inventory.hidden ?? [])
  const needle = query.trim().toLowerCase()
  const router = inventory.providers.find((provider) => provider.source === 'router') ?? null
  const groups = inventory.providers
    .filter((provider) => provider.source !== 'router')
    .map((provider) => ({
      id: provider.id,
      name: provider.name,
      source: provider.source,
      classifier: '',
      models: [...new Map((provider.models ?? []).map((model) => [model.id, model])).values()]
        .filter((model) => !hidden.has(modelKey(provider.id, model.id)))
        .map((model) => ({ ...model, label: model.id, provider: provider.id, choice: model.id, accounts: 0 })),
    }))
  if (router) {
    for (const entry of inventory.router_models ?? []) {
      const catalogId = FAMILY_CATALOG[entry.family] ?? entry.family
      let group = groups.find((candidate) => candidate.id === `router:${entry.family}` || connectedCatalogId(candidate.id) === catalogId)
      if (!group) {
        group = { id: `router:${entry.family}`, name: FAMILY_NAMES[entry.family] ?? entry.family, source: 'router', classifier: '', models: [] }
        groups.push(group)
      }
      if (hidden.has(modelKey(router.id, entry.id)) || hidden.has(modelKey(group.id, entry.model))) {
        group.models = group.models.filter((model) => model.id !== entry.model)
        continue
      }
      const existing = group.models.find((model) => model.id === entry.model)
      if (existing) {
        existing.accounts = entry.accounts
        if (entry.accounts > 0) {
          existing.provider = router.id
          existing.choice = entry.id
        }
      } else {
        group.models.push({ id: entry.model, context: '', label: entry.model, provider: router.id, choice: entry.id, accounts: entry.accounts })
      }
    }
    if (inventory.router_classifier || inventory.router_models?.length) {
      groups.unshift({
        id: router.id,
        name: router.name,
        source: 'router',
        classifier: inventory.router_classifier || 'Automatic',
        models: [{ id: 'auto', context: '', label: inventory.router_classifier ? `${inventory.router_classifier} picks` : 'Automatic', provider: router.id, choice: 'auto', accounts: 0 }],
      })
    }
  }
  return groups
    .map((group) => ({
      ...group,
      models: group.models.filter((model) => !needle || model.id.toLowerCase().includes(needle) || model.label.toLowerCase().includes(needle) || group.name.toLowerCase().includes(needle)),
    }))
    .filter((group) => group.models.length > 0)
}

// The row in use: the saved default when a row still saves it, else the first
// row, else nothing. The answer names the provider and the choice the row saves.
export function currentModel(inventory) {
  if (!inventory) return null
  const rows = pickerGroups(inventory).flatMap((group) => group.models)
  const saved = inventory.default_provider && inventory.default_model
    ? rows.find((row) => row.provider === inventory.default_provider && row.choice === inventory.default_model)
    : null
  const sourceGroup = pickerGroups(inventory).find((group) => group.id === inventory.default_provider)
  const equivalent = sourceGroup?.models.find((row) => row.id === inventory.default_model)
  const row = saved ?? equivalent ?? rows[0]
  return row ? { provider: row.provider, model: row.choice, label: row.label } : null
}

// Router catalog entries remain visible when their account pool is empty.
// A direct provider has already supplied its configured models in inventory.
export function currentModelAvailable(inventory) {
  const current = currentModel(inventory)
  if (!current) return false
  const provider = inventory.providers.find((entry) => entry.id === current.provider)
  if (provider?.source !== 'router') return true
  return (inventory.router_models ?? []).some((entry) => entry.accounts > 0
    && (current.model === 'auto' || entry.id === current.model))
}

// The composer chip: the model id in use beside the provider's mark, `Connect a
// model` when none answers.
export function modelChipLabel(inventory) {
  const current = currentModel(inventory)
  if (!current) return 'Connect a model'
  return current.label
}

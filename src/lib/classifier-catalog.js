// The classifier catalog: the models that can pick a route per turn.
//
// Suggestions contain dedicated decision models and compatible endpoints.
// General chat models stay available for answers, not classifier suggestions.
// Existing pooled classifier settings remain readable.
//
// A price is per million input tokens, in US dollars, as the provider lists
// it. It is a guide for picking, never a bill.

// Models built to classify. Each takes its own key.
export const DEDICATED = [
  {
    id: 'typesafe/jev-latest',
    kind: 'typesafe',
    name: 'TypeSafe Jev',
    model: 'jev-latest',
    note: 'A System One model. It answers a typed choice with a probability over every model in the running, not prose.',
    price: 0.042,
    context: '64K',
  },
]

// Open models use an operator-hosted System One endpoint.
export const SELF_HOSTED = [
  {
    id: 'semif/qwen3.5-4b', kind: 'endpoint', name: 'SemIf · Qwen 3.5 4B', model: 'semif-qwen3.5-4b',
    note: 'A self-hosted routing classifier. Requires a System One adapter. Scores come from model logits and need calibration for your routes.',
  },
  {
    id: 'mapika/decider-2b', kind: 'endpoint', name: 'Decider 2B', model: 'decider-2b',
    note: 'A self-hosted routing classifier. Requires a System One endpoint. Faster on the tested GPU, but less accurate than Jev on complex decisions.',
  },
]

// Identify saved pooled classifiers without recommending them.
const LEGACY_POOLED = [
  { id: 'openai/gpt-5.6-luna', family: 'openai', model: 'gpt-5.6-luna', name: 'GPT 5.6 Luna', price: 0.2 },
  { id: 'google/gemini-3.5-flash-lite', family: 'google', model: 'gemini-3.5-flash-lite', name: 'Gemini 3.5 Flash Lite', price: 0.15 },
  { id: 'anthropic/claude-haiku-4-5', family: 'anthropic', model: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', price: 1 },
  { id: 'xai/grok-4.6', family: 'xai', model: 'grok-4.6', name: 'Grok 4.6', price: 2 },
  { id: 'kimi/kimi-k3', family: 'kimi', model: 'kimi-k3', name: 'Kimi K3', price: 3 },
]

// The price line one catalog row shows.
export function priceLabel(price) {
  if (!Number.isFinite(price)) return 'Price unavailable'
  if (price <= 0) return 'free'
  if (price < 1) return `$${price.toFixed(3).replace(/0+$/, '').replace(/\.$/, '')}/M in`
  return `$${price}/M in`
}

// The catalog row a saved classifier matches, so the screen marks it.
export function matchSaved(classifier) {
  if (!classifier || classifier.kind === 'none') return ''
  if (classifier.kind === 'typesafe') {
    return DEDICATED.find((entry) => entry.kind === 'typesafe')?.id ?? ''
  }
  if (classifier.kind === 'pooled') return `${classifier.family}/${classifier.model}`
  return SELF_HOSTED.find(entry => entry.model === classifier.model)?.id ?? 'endpoint'
}

// Every catalog row in the order the screen lists them.
export function catalog() {
  return [
    ...DEDICATED.map((entry) => ({ ...entry, ready: true, group: 'Built to classify' })),
    ...SELF_HOSTED.map(entry => ({ ...entry, ready: true, group: 'Self-hosted' })),
  ]
}

// Runtime inventories may provide either a qualified ID or the model alone.
export function classifierProvider(model) {
  const entry = [...DEDICATED, ...SELF_HOSTED, ...LEGACY_POOLED].find(item => item.id === model || item.model === model)
  return (entry?.id ?? model ?? '').split('/')[0]
}

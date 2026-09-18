// The classifier catalog: the models that can pick a route per turn.
//
// A classifier reads the turn and names one route. Three kinds reach one:
// a model built to classify, behind its own key; a small model on an account
// the pools already hold, which spends what the user already pays for; and any
// server that answers the same choice question.
//
// A price is per million input tokens, in US dollars, as the provider lists
// it. It is a guide for picking, never a bill.

// Models built to classify. Each takes its own key.
export const DEDICATED = [
  {
    id: 'typesafe/jev-latest',
    kind: 'typesafe',
    name: 'TypeSafe jev',
    model: 'jev-latest',
    note: 'A System One model. It answers a typed choice with a probability over every model in the running, not prose.',
    price: 0.042,
    context: '64K',
  },
]

// Small models that classify well on an account a pool already holds. The
// router asks for one JSON object and reads the choice out of it.
export const POOLED = [
  { id: 'openai/gpt-5.6-luna', family: 'openai', model: 'gpt-5.6-luna', name: 'GPT-5.6 Luna', price: 0.2 },
  { id: 'google/gemini-3.5-flash-lite', family: 'google', model: 'gemini-3.5-flash-lite', name: 'Gemini 3.5 Flash-Lite', price: 0.15 },
  { id: 'anthropic/claude-haiku-4-5', family: 'anthropic', model: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', price: 1 },
  { id: 'xai/grok-4.6', family: 'xai', model: 'grok-4.6', name: 'Grok 4.6', price: 2 },
  { id: 'kimi/kimi-k3', family: 'kimi', model: 'kimi-k3', name: 'Kimi K3', price: 3 },
]

// The price line one catalog row shows.
export function priceLabel(price) {
  if (!Number.isFinite(price) || price <= 0) return 'free'
  if (price < 1) return `$${price.toFixed(3).replace(/0+$/, '').replace(/\.$/, '')}/M in`
  return `$${price}/M in`
}

// Whether a pooled classifier can run: its family needs an account in the pool.
export function pooledReady(entry, accounts = []) {
  return accounts.some((account) => account.family === entry.family && account.enabled)
}

// The catalog row a saved classifier matches, so the screen marks it.
export function matchSaved(classifier) {
  if (!classifier || classifier.kind === 'none') return ''
  if (classifier.kind === 'typesafe') {
    return DEDICATED.find((entry) => entry.kind === 'typesafe')?.id ?? ''
  }
  if (classifier.kind === 'pooled') return `${classifier.family}/${classifier.model}`
  return 'endpoint'
}

// Every catalog row in the order the screen lists them.
export function catalog(accounts = []) {
  return [
    ...DEDICATED.map((entry) => ({ ...entry, ready: true, group: 'Built to classify' })),
    ...POOLED.map((entry) => ({ ...entry, kind: 'pooled', ready: pooledReady(entry, accounts), group: 'On your accounts' })),
  ]
}

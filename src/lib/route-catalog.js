// The models a route can name, per provider family.
//
// The classifier picks between models the pools already serve, so the routes
// editor offers this list rather than a blank field. A model the list does not
// carry is still typeable: the list is a starting point, never a fence.
//
// A price is per million input tokens, in US dollars, as the provider lists
// it. `deep` is the family's model for hard, long work. `fast` is its cheap
// one. A family's `route` model is the one a suggested route names.

export const ROUTE_MODELS = {
  openai: [
    { model: 'gpt-6-astra', name: 'GPT-6 Astra', tier: 'deep', price: 10, context: '1.05M', route: true },
    { model: 'gpt-5.6-nano', name: 'GPT-5.6 nano', tier: 'fast', price: 0.05, context: '400K' },
  ],
  anthropic: [
    { model: 'claude-opus-5', name: 'Claude Opus 5', tier: 'deep', price: 15, context: '1M', route: true },
    { model: 'claude-sonnet-5', name: 'Claude Sonnet 5', tier: 'balanced', price: 3, context: '1M' },
    { model: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', tier: 'fast', price: 1, context: '200K' },
  ],
  google: [
    { model: 'gemini-3.1-pro', name: 'Gemini 3.1 Pro', tier: 'deep', price: 2, context: '1M', route: true },
    { model: 'gemini-3.5-flash', name: 'Gemini 3.5 Flash', tier: 'balanced', price: 1.5, context: '1M' },
    { model: 'gemini-3.5-flash-lite', name: 'Gemini 3.5 Flash-Lite', tier: 'fast', price: 0.15, context: '1M' },
  ],
  xai: [
    { model: 'grok-4.6', name: 'Grok 4.6', tier: 'deep', price: 2, context: '500K', route: true },
  ],
  kimi: [
    { model: 'kimi-k3', name: 'Kimi K3', tier: 'deep', price: 3, context: '1M', route: true },
  ],
}

// What a suggested route says it is for. The classifier reads these, so each
// one names the work, never the model.
const DESCRIPTIONS = {
  openai: 'A long multi-step task: end-to-end coding, driving tools, research that runs for many steps',
  anthropic: 'A hard reasoning task: a subtle bug, a plan to weigh, writing that has to be right',
  google: 'A task over a large body of text: a long document, a wide codebase, many files at once',
  xai: 'A task about what is happening now: current events, live search, a post or a feed',
  kimi: 'A long open-ended task on an open model: bulk work where cost matters more than the last point of quality',
}
const FAST_DESCRIPTION = 'A short question: a lookup, a one-line edit, a yes or no, a rename'

// Every model of one family, newest and most capable first.
export function familyModels(family) {
  return ROUTE_MODELS[family] ?? []
}

// The model a suggested route names for this family.
export function routeModel(family) {
  const models = familyModels(family)
  return models.find((entry) => entry.route) ?? models[0] ?? null
}

// The catalog entry for one family and model, when the list carries it.
export function findModel(family, model) {
  return familyModels(family).find((entry) => entry.model === model) ?? null
}

// The families a pool can serve right now: one enabled account is enough.
export function connectedFamilies(accounts = []) {
  const seen = []
  for (const account of accounts) {
    if (account.enabled && !seen.includes(account.family)) seen.push(account.family)
  }
  return seen
}

// Whether a route can be served: its family needs an enabled account.
export function routeReady(route, accounts = []) {
  return connectedFamilies(accounts).includes(route.family)
}

// A route set built from the pools as they stand: the cheapest connected model
// takes the short questions, and every connected family takes the work it is
// best at. A user edits these; nothing here is fixed.
export function suggestRoutes(accounts = []) {
  const families = connectedFamilies(accounts)
  if (families.length === 0) return []
  const routes = []
  const cheapest = families
    .flatMap((family) => familyModels(family).map((entry) => ({ ...entry, family })))
    .filter((entry) => entry.tier === 'fast')
    .sort((left, right) => left.price - right.price)[0]
  if (cheapest) {
    routes.push({ key: 'fast', description: FAST_DESCRIPTION, family: cheapest.family, model: cheapest.model })
  }
  for (const family of families) {
    const entry = routeModel(family)
    if (!entry) continue
    if (routes.some((route) => route.family === family && route.model === entry.model)) continue
    routes.push({ key: family, description: DESCRIPTIONS[family] ?? `Work for ${family}`, family, model: entry.model })
  }
  return routes
}

// The route a suggested set falls back to: the short-question route when there
// is one, else the first route.
export function suggestedFallback(routes) {
  return routes.find((route) => route.key === 'fast')?.key ?? routes[0]?.key ?? ''
}

import { modelLabel } from './model-label.js'
import { leadingModels, newestFirst } from './model-recency.js'
import { isDecisionModel } from './classifier-connections.js'
import { registry } from './model-registry.js'
// The provider catalog behind Settings → Models: Pi's built-in providers, the
// method each one offers, and the helpers the section and the picker share.
// The model registry supplies the providers and their names. `account` names
// the Pi provider that signs in with an account and the label the method
// shows. Anthropic's account is the Claude Code sign-in through the
// pi-claude-bridge package, so it carries no Pi OAuth provider here.

export const PROVIDERS = registry.providers

const METHOD_LABELS = registry.methodLabels

// Pi ids that reach the shell under another provider's name: the Codex account
// is OpenAI, the bridge is Anthropic.
const PROVIDER_ALIASES = registry.aliases

// The catalog id a connected provider edits under.
export function catalogId(id) {
  return PROVIDER_ALIASES[id] ?? id
}

export function catalogProvider(id) {
  return PROVIDERS.find((provider) => provider.id === id) ?? null
}

export function providerName(id, inventory = null) {
  const connected = inventory?.providers?.find((provider) => provider.id === id)
  if (connected?.name) return connected.name
  return catalogProvider(catalogId(id))?.name ?? id
}

export function methodLabel(provider, method) {
  if (method === 'account') return provider.account?.label ?? METHOD_LABELS.account
  return METHOD_LABELS[method] ?? method
}

// The catalog split the connector shows: the featured providers first, then the rest
// by name. A query narrows both by name or id.
export function searchProviders(query = '') {
  const needle = query.trim().toLowerCase()
  const matches = PROVIDERS.filter((provider) => !needle
    || provider.name.toLowerCase().includes(needle)
    || provider.id.includes(needle)
    || provider.methods.some((method) => methodLabel(provider, method).toLowerCase().includes(needle))
    || (provider.account?.provider ?? '').includes(needle))
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

export const SOURCE_TAGS = registry.sourceTags

export function sourceTag(source) {
  return SOURCE_TAGS[source] ?? source
}

export function modelKey(provider, model) {
  return `${provider}/${model}`
}

// The router's families under the catalog provider each one is, and the name
// a family group takes when no provider of its own is connected.
const FAMILY_CATALOG = registry.familyProviders
const FAMILY_NAMES = registry.familyNames

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

// The picker's rows: every shown model of every connected provider, grouped by
// provider in inventory order with each provider's newest model first, narrowed
// by a query over the model id. Each row
// names the provider and the choice a pick saves, so a row the router serves
// saves the router's own id.
//
// A model is shown when it leads its provider's list, the newest of its family,
// or when the user turned it on, unless the user turned it off. The saved
// default stays shown, so the chip always names a row. With `all`, every row
// comes back, each marked with whether it leads and whether it is shown.
//
// The router's models sit under their provider, each model once: a model the
// provider also serves directly is one row, and it takes the router's route
// when two or more accounts stand behind it, so picking it balances across
// them with no other step. A family with no provider of its own becomes a
// group of its own. A running classifier is the first row of the picker,
// named by the model that picks, and choosing it hands the turn to
// classification, which is the router's auto.
export function pickerGroups(inventory, query = '', { all = false } = {}) {
  if (!inventory || !Array.isArray(inventory.providers)) return []
  const hidden = new Set(inventory.hidden ?? [])
  const turnedOn = new Set(inventory.shown ?? [])
  const needle = query.trim().toLowerCase()
  const router = inventory.providers.find((provider) => provider.source === 'router') ?? null
  const groups = inventory.providers
    .filter((provider) => provider.source !== 'router')
    .map((provider) => ({
      id: provider.id,
      name: provider.name,
      source: provider.source,
      classifier: '',
      // An Ollama decision model answers only the router's choice, so it lists with the classifiers.
      models: [...new Map((provider.models ?? []).map((model) => [model.id, model])).values()]
        .filter((model) => !isDecisionModel(provider, model.id))
        .map((model) => ({ ...model, label: modelLabel(model.id), provider: provider.id, choice: model.id, accounts: 0, off: hidden.has(modelKey(provider.id, model.id)) })),
    }))
  if (router) {
    for (const entry of inventory.router_models ?? []) {
      const catalogId = FAMILY_CATALOG[entry.family] ?? entry.family
      let group = groups.find((candidate) => candidate.id === `router:${entry.family}` || connectedCatalogId(candidate.id) === catalogId)
      if (!group) {
        group = { id: `router:${entry.family}`, name: FAMILY_NAMES[entry.family] ?? entry.family, source: 'router', classifier: '', models: [] }
        groups.push(group)
      }
      const off = hidden.has(modelKey(router.id, entry.id)) || hidden.has(modelKey(group.id, entry.model))
      const existing = group.models.find((model) => model.id === entry.model)
      if (existing) {
        existing.accounts = entry.accounts
        existing.off ||= off
        if (entry.accounts > 0) {
          existing.provider = router.id
          existing.choice = entry.id
        }
      } else {
        group.models.push({ id: entry.model, context: '', contextWindow: entry.context_window ?? null, label: modelLabel(entry.model), provider: router.id, choice: entry.id, accounts: entry.accounts, off })
      }
    }
    // Automatic routes each message through a decision model, so its row
    // appears only once one is connected. Until routing is on, the Model router
    // heading carries a Manage link to Decision models. Pooled models balance
    // their accounts either way.
    if (inventory.router_classifier || inventory.router_models?.length) {
      const routing = !!inventory.router_classifier && inventory.default_provider === router.id && inventory.default_model === 'auto'
      groups.unshift({
        id: router.id,
        name: router.name,
        source: 'router',
        classifier: inventory.router_classifier || 'Automatic',
        manage: !routing,
        models: inventory.router_classifier ? [{ id: 'auto', context: '', label: `${modelLabel(inventory.router_classifier)} picks`, provider: router.id, choice: 'auto', accounts: 0, off: false }] : [],
      })
    }
  }
  for (const group of groups) {
    if (!group.classifier) {
      const order = newestFirst(groupCatalogId(group.id), group.models.map((model) => model.id))
      group.models.sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id))
    }
    const leading = group.classifier ? null : leadingModels(groupCatalogId(group.id), group.models.map((model) => model.id))
    for (const model of group.models) {
      const saved = (inventory.default_provider === model.provider && inventory.default_model === model.choice)
        || (inventory.default_provider === group.id && inventory.default_model === model.id)
      model.lead = !leading || leading.has(model.id)
      model.shown = !model.off && (model.lead || saved || turnedOn.has(modelKey(model.provider, model.choice)) || turnedOn.has(modelKey(group.id, model.id)))
    }
  }
  return groups
    .map((group) => ({
      ...group,
      models: group.models
        .filter((model) => (all || model.shown) && (!needle || model.id.toLowerCase().includes(needle) || model.label.toLowerCase().includes(needle) || group.name.toLowerCase().includes(needle)))
        .map(({ off, lead, shown, ...model }) => all ? { ...model, lead, shown } : model),
    }))
    // The router heading stays in the picker for its Manage link while routing is off.
    .filter((group) => group.models.length > 0 || (group.manage && !needle && !all))
}

// The router models the picker hides, as the router's sorted `family/model`
// keys. The router leaves them out of the running, so a routed turn answers
// only from a model the picker offers.
export function routerHidden(inventory) {
  const groups = pickerGroups(inventory, '', { all: true })
  return [...new Set((inventory?.router_models ?? []).filter((entry) => {
    const group = groups.find((candidate) => groupCatalogId(candidate.id) === familyProvider(entry.family))
    const row = group?.models.find((model) => model.id === entry.model)
    return row && !row.shown
  }).map((entry) => `${entry.family}/${entry.model}`))].sort()
}

// The catalog provider a picker group's models come from, which names its
// models.dev listing.
function groupCatalogId(id) {
  return id.startsWith('router:') ? familyProvider(id.slice('router:'.length)) : connectedCatalogId(id)
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

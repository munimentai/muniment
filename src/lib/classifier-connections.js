import { registry } from './model-registry.js'

// Curated decision-model connections, from the model registry. Ordering favors
// adoption and ease of setup. Each entry answers System One's typed choice.
// Pi's model catalog names the hosted ids, and each project's own server names
// the local ones. A connection method with `ollama` reaches a decision model on
// the connected Ollama server, under Ollama's own System One route.
export const CLASSIFIERS = registry.classifiers
export const CONNECTION_METHODS = registry.connectionMethods
export const LOCAL_SETUP = registry.localSetup
export function searchClassifiers(query = '') {
  const needle = query.trim().toLowerCase()
  return CLASSIFIERS.filter(row => `${row.name} ${row.note}`.toLowerCase().includes(needle))
}

// Ollama's decision models, by the name Ollama gives each, with the catalog
// entry whose mark each shows: https://ollama.com/search?c=decision. Ollama's
// model list does not mark them, so for Ollama the name is the fallback.
const OLLAMA_DECISIONS = registry.ollamaDecisions

// Whether a provider's model is a decision model. It answers a typed choice,
// never a chat turn, so it belongs with the classifiers and not in the model
// picker. A server such as llama.cpp reports its decision models itself.
export function isDecisionModel(provider, id) {
  if (provider?.decision_models?.includes(id)) return true
  return provider?.id === 'ollama' && Object.hasOwn(OLLAMA_DECISIONS, String(id).split(':')[0])
}

// The decision models each connected server serves and no saved connection
// uses yet. Each answers on the server's own System One route.
export function serverDecisionModels(inventory, connections = []) {
  return (inventory?.providers ?? []).flatMap(server => {
    if (!server.base_url) return []
    const baseUrl = `${server.base_url.replace(/\/+$/, '')}/systemone`
    const ids = [...new Set([...(server.decision_models ?? []), ...(server.models ?? []).map(({ id }) => id).filter(id => isDecisionModel(server, id))])]
    return ids.flatMap(id => {
      if (connections.some(entry => entry.connection?.model === id && entry.connection?.base_url === baseUrl)) return []
      const known = OLLAMA_DECISIONS[id.split(':')[0]]
      return [{ model: id, provider: server.id, catalogId: known?.[0] ?? server.id, name: `${known?.[1] ?? id} · ${server.name ?? server.id}`, baseUrl }]
    })
  })
}

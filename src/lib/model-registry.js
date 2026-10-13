// The provider and model facts the shell shows, from the runtime's model
// registry. Startup loads them once, before the interface mounts. Each list and
// table is filled in place, so a module that holds one reads the loaded data.

export const registry = {
  providers: [],
  methodLabels: {},
  aliases: {},
  sourceTags: {},
  // The provider each router family pools from, and the name each family shows.
  familyProviders: {},
  familyNames: {},
  subscriptions: [],
  classifiers: [],
  connectionMethods: {},
  localSetup: {},
  // Ollama's decision models: the classifier whose mark each shows and its name.
  ollamaDecisions: {},
}

function fill(target, source) {
  if (Array.isArray(target)) target.splice(0, target.length, ...(source ?? []))
  else {
    for (const key of Object.keys(target)) delete target[key]
    Object.assign(target, source ?? {})
  }
}

// Puts one registry answer in place.
export function installModelRegistry(data) {
  const families = data?.families ?? []
  fill(registry.providers, data?.providers)
  fill(registry.methodLabels, data?.methodLabels)
  fill(registry.aliases, data?.aliases)
  fill(registry.sourceTags, data?.sourceTags)
  fill(registry.familyProviders, Object.fromEntries(families.filter((family) => family.provider).map((family) => [family.id, family.provider])))
  fill(registry.familyNames, Object.fromEntries(families.map((family) => [family.id, family.name])))
  fill(registry.subscriptions, data?.subscriptions)
  fill(registry.classifiers, data?.classifiers?.entries)
  fill(registry.connectionMethods, data?.classifiers?.methods)
  fill(registry.localSetup, data?.classifiers?.localSetup)
  fill(registry.ollamaDecisions, data?.classifiers?.ollamaDecisions)
}

let loading = null

// Reads the registry from the runtime once. Outside the app shell, or when the
// read fails, the lists stay empty and the interface still renders.
export function loadModelRegistry(tauri = window.__TAURI__?.core) {
  loading ??= Promise.resolve(tauri?.invoke('model_registry'))
    .then((data) => { if (data) installModelRegistry(data) })
    .catch(() => {})
  return loading
}

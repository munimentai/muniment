// Curated decision-model connections. Ordering favors adoption and ease of setup.
// Each entry answers System One's typed choice. Pi's model catalog names the
// hosted ids, and each project's own server names the local ones.
const local = (id, name, repo, model, note, compatible = true, methods = ['server', 'download']) => ({
  id, name, repo: `https://github.com/${repo}`, model, note, compatible, methods,
})
const entries = [
  { id: 'jev', name: 'Jev', model: 'jev-latest', methods: ['typesafe', 'openrouter', 'cloudflare', 'server'], note: 'Hosted decision model. Choose where your routing requests go.' },
  // Cloudflare's decision models, in two sizes. Each way to connect names the
  // model by its own id: Cloudflare Workers AI, an Ollama server, or OpenRouter.
  {
    id: 'clef', name: 'Clef', model: 'clef-flash', methods: ['cloudflare', 'ollama', 'openrouter'],
    note: "Cloudflare's decision models. Clef Flash is the fast, inexpensive one. Clef is the larger, more accurate one.",
    variants: [
      { id: 'clef-flash', name: 'Clef Flash', models: { cloudflare: '@cf/cloudflare/clef-flash', ollama: 'clef-flash', openrouter: 'cloudflare/clef-flash' } },
      { id: 'clef', name: 'Clef', models: { cloudflare: '@cf/cloudflare/clef', ollama: 'clef', openrouter: 'cloudflare/clef' } },
    ],
  },
  // OpenAI's decision model answers through the Decisions API, which takes an
  // OpenAI API key. A ChatGPT sign-in does not reach it.
  { id: 'luna', name: 'GPT-6 Luna', model: 'gpt-6-luna', methods: ['openai'], note: "OpenAI's decision model. It needs an OpenAI API key, because a ChatGPT sign-in does not reach the Decisions API." },
  local('kev', 'Kev', 'jaredpalmer/kev', 'kev-latest', 'Local decision models in several sizes. The server downloads the checkpoint and base model on first use.'),
  local('nimble', 'Nimble', 'bespokelabsai/nimble', 'nimble', 'A 9B decision model. Ollama serves it, or run its own System One server.', true, ['ollama', 'server', 'download']),
  local('von', 'Von', 'wfzyx/von', 'von-1.3.0', 'A small English decision model that runs on a CPU.'),
  local('laya', 'Laya', 'NandhaKishorM/laya', 'english', 'Very fast on a CPU, and less accurate at picking a model. The server downloads weights on first use.'),
  local('decider', 'Decider 4B', 'Mapika/decider', 'Mapika/decider-4b', 'Runs on CPU, Apple Silicon or CUDA. The 4B weights need about 8 GB in bfloat16.'),
]
const order = ['jev', 'clef', 'luna', 'kev', 'nimble', 'von', 'laya', 'decider']
export const CLASSIFIERS = order.map(id => entries.find(entry => entry.id === id))
export const CONNECTION_METHODS = {
  typesafe: { name: 'TypeSafe API', url: 'https://api.typesafe.ai/v1/systemone', model: 'jev-latest', key: true },
  openrouter: { name: 'OpenRouter API', url: 'https://openrouter.ai/api/v1/systemone', model: 'typesafe/jev-1.13', key: true },
  openai: { name: 'OpenAI API', url: 'https://api.openai.com/v1/decisions', model: 'gpt-6-luna', key: true },
  cloudflare: { name: 'Cloudflare Workers AI', model: 'typesafe/jev', key: true, guide: 'https://developers.cloudflare.com/ai/models/typesafe/jev/' },
  // A decision model on the connected Ollama server, under Ollama's own System One route.
  ollama: { name: 'Ollama server', ollama: true },
  server: { name: 'Connect a server', url: 'http://127.0.0.1:8000/v1/systemone' },
  download: { name: 'Download and run locally' },
}
export const LOCAL_SETUP = {
  laya: 'pip install "laya[serve]"\nLAYA_MODELS=english laya-serve',
  kev: 'git clone https://github.com/jaredpalmer/kev.git\ncd kev\nuv sync --extra serve\nuv run --extra serve python -m kev.serve --run jaredpalmer/kev-4b --port 8009',
  decider: 'git clone https://github.com/Mapika/decider.git\ncd decider\npip install -e ".[serve]"\nscripts/serve.sh Mapika/decider-4b 8000',
  von: 'pip install von-sdk\nvon serve --host 127.0.0.1 --port 8000',
}
export function searchClassifiers(query = '') {
  const needle = query.trim().toLowerCase()
  return CLASSIFIERS.filter(row => `${row.name} ${row.note}`.toLowerCase().includes(needle))
}

// Ollama's decision models, by the name Ollama gives each, with the catalog
// entry whose mark each shows: https://ollama.com/search?c=decision. Ollama's
// model list does not mark them, so for Ollama the name is the fallback.
const OLLAMA_DECISIONS = {
  'clef-flash': ['clef', 'Clef Flash'],
  clef: ['clef', 'Clef'],
  nimble: ['nimble', 'Nimble'],
  laya: ['laya', 'Laya'],
  tev1: ['tev1', 'Tev1'],
}

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

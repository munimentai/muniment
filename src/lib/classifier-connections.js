// Curated decision-model connections. Ordering favors adoption and ease of setup.
// Source: https://www.datacamp.com/blog/top-open-source-jev-alternatives
const local = (id, name, repo, model, note, compatible = true) => ({
  id, name, repo: `https://github.com/${repo}`, model, note, compatible,
  methods: ['server', 'download'],
})
const entries = [
  { id: 'jev', name: 'Jev', model: 'jev-latest', methods: ['typesafe', 'openrouter', 'cloudflare', 'server'], note: 'Hosted decision model. Choose where your routing requests go.' },
  local('needle', 'Needle 3', 'cactus-compute/needle', 'needle3', 'Small local engine. Its tool-call API needs a System One adapter before Muniment can connect.', false),
  local('laya', 'Laya', 'NandhaKishorM/laya', 'english', 'Small multilingual decision models. The server downloads weights on first use.'),
  local('kev', 'Kev', 'jaredpalmer/kev', 'kev-latest', 'Local decision models in several sizes. The server downloads the checkpoint and base model on first use.'),
  local('nimble', 'Nimble', 'bespokelabsai/nimble', 'bespokelabs/Bespoke-Nimble-9B', 'A 9B decision model. Requires an NVIDIA GPU setup and a System One adapter.', false),
  local('semif', 'SemIf', 'TheoLeeCJ/SemIf-OpenJev', 'semif-qwen3.5-4b', 'Scores decisions with open models. Requires a System One adapter.', false),
  local('decider', 'Decider 2B', 'Mapika/decider', 'Mapika/decider-2b', 'Runs on CPU, Apple Silicon or CUDA. The 2B weights need about 4 GB in bfloat16.'),
  local('rizzo', 'Rizzo Flow', 'Rizzo-AI-Academy/rizzo-flow', 'rizzo-flow', 'Local decision engine. Requires a System One adapter.', false),
  local('von', 'Von', 'wfzyx/von', 'von-1.2.0', 'Local decision model with a System One server.'),
  local('nanojev', 'NanoJev', 'TianyuCodings/NanoJev', 'nanojev', 'Research checkpoint trained on game decisions. Needs task evaluation and a System One adapter.', false),
]
const order = ['jev', 'laya', 'needle', 'kev', 'semif', 'nanojev', 'nimble', 'von', 'rizzo', 'decider']
export const CLASSIFIERS = order.map(id => entries.find(entry => entry.id === id))
export const CONNECTION_METHODS = {
  typesafe: { name: 'TypeSafe API', url: 'https://api.typesafe.ai/v1/systemone', model: 'jev-latest', key: true },
  openrouter: { name: 'OpenRouter API', url: 'https://openrouter.ai/api/v1/systemone', model: 'typesafe/jev-1.13', key: true },
  cloudflare: { name: 'Cloudflare Workers AI', model: 'typesafe/jev', key: true, guide: 'https://developers.cloudflare.com/ai/models/typesafe/jev/' },
  server: { name: 'Connect a server', url: 'http://127.0.0.1:8000/v1/systemone' },
  download: { name: 'Download and run locally' },
}
export const LOCAL_SETUP = {
  laya: 'pip install "laya[serve]"\nlaya-serve',
  kev: 'git clone https://github.com/jaredpalmer/kev.git\ncd kev\nuv sync --extra serve\nuv run --extra serve python -m kev.serve --run jaredpalmer/kev-4b --port 8009',
  decider: 'git clone https://github.com/Mapika/decider.git\ncd decider\npip install -e ".[serve]"\nscripts/serve.sh Mapika/decider-2b 8000',
  von: 'pip install von-sdk\nvon serve --host 127.0.0.1 --port 8000',
}
export function searchClassifiers(query = '') {
  const needle = query.trim().toLowerCase()
  return CLASSIFIERS.filter(row => `${row.name} ${row.note}`.toLowerCase().includes(needle))
}

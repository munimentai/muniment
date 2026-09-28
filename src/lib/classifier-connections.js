// Recommendations follow the published Muniment policy-adherence evaluation.
const local = (id, name, repo, model, note, compatible = true) => ({
  id, name, repo: `https://github.com/${repo}`, model, note, compatible,
  methods: ['server', 'download'],
})
const entries = [
  { id: 'jev', name: 'Jev', model: 'jev-latest', methods: ['typesafe', 'openrouter', 'cloudflare', 'server'], note: 'Hosted decision model. Choose where your routing requests go.' },
  local('needle', 'Needle 3', 'cactus-compute/needle', 'needle3', 'Small local engine. Its tool-call API needs a System One adapter before Muniment can connect.', false),
  local('laya', 'Laya', 'NandhaKishorM/laya', 'english', 'Small multilingual decision models. The server downloads weights on first use.'),
  local('kev', 'Kev 4B', 'jaredpalmer/kev', 'kev-latest', 'Local decision models in several sizes. The server downloads the checkpoint and base model on first use.'),
  local('nimble', 'Nimble', 'bespokelabsai/nimble', 'bespokelabs/Bespoke-Nimble-9B', 'A 9B decision model with CUDA and Apple Silicon backends. Connect through its System One server.'),
  local('semif', 'SemIf', 'TheoLeeCJ/SemIf-OpenJev', 'semif-qwen3.5-4b', 'Scores decisions with open models. Requires a System One adapter.', false),
  local('decider', 'Decider 2B', 'Mapika/decider', 'Mapika/decider-2b', 'Runs on CPU, Apple Silicon or CUDA. The 2B weights need about 4 GB in bfloat16.'),
  local('rizzo', 'Rizzo Flow', 'Rizzo-AI-Academy/rizzo-flow', 'rizzo-flow', 'Local decision engine. Requires a System One adapter.', false),
  local('von', 'Von', 'wfzyx/von', 'von-1.2.0', 'Local decision model with a System One server.'),
  local('nanojev', 'NanoJev', 'TianyuCodings/NanoJev', 'nanojev', 'Research checkpoint trained on game decisions. Needs task evaluation and a System One adapter.', false),
]
const order = ['jev', 'kev', 'nimble', 'laya', 'needle', 'semif', 'nanojev', 'von', 'rizzo', 'decider']
const recommendations = { jev: 'Top choice', kev: 'Preferred', nimble: 'Preferred' }
export const CLASSIFIERS = order.map(id => ({ ...entries.find(entry => entry.id === id), badge: recommendations[id] }))
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

export const CLASSIFIER_GUIDES = {
  jev: {
    steps: ['Create a TypeSafe account and an API key, or choose another listed provider.', 'Select the provider that issued your key. Keep its model ID and endpoint together.', 'Paste the key, select Test and connect, then select Jev in Routing.'],
    links: [{ label: 'Get a TypeSafe API key', url: 'https://console.typesafe.ai/' }, { label: 'TypeSafe API documentation', url: 'https://docs.typesafe.ai/' }, { label: 'Jev on OpenRouter', url: 'https://openrouter.ai/typesafe/jev-1.13' }],
    detail: 'Jev is hosted. Routing context leaves this device. Top choice reflects a small policy test, not guaranteed task success.',
  },
  kev: {
    steps: ['Install Git, Python 3.12 and uv. Run the Kev 4B server with the commands below.', 'Keep the server running. Its first start downloads the adapter and base weights.', 'Connect to port 8009 using model ID kev-latest. This ID serves the checkpoint selected by the server command.'],
    links: [{ label: 'Kev setup guide', url: 'https://github.com/jaredpalmer/kev#readme' }, { label: 'Kev 4B weights', url: 'https://huggingface.co/jaredpalmer/kev-4b' }],
    detail: 'Kev supports Apple Silicon through MLX and NVIDIA through PyTorch. Allow memory for the base model and inference buffers.',
  },
  nimble: {
    steps: ['Follow the Nimble setup guide to prepare the trained Bespoke-Nimble-9B checkpoint.', 'Use the upstream System One deployment guide, or use its public hosted endpoint for a first connection.', 'Enter the full /v1/systemone URL and model ID nimble-latest. The hosted service receives your routing context.'],
    links: [{ label: 'System One deployment guide', url: 'https://github.com/bespokelabsai/nimble/blob/main/docs/MODAL_SERVING.md' }, { label: 'Public endpoint instructions', url: 'https://github.com/bespokelabsai/nimble/blob/main/docs/TRY_NIMBLE.md' }, { label: 'Nimble setup guide', url: 'https://github.com/bespokelabsai/nimble#quickstart' }, { label: 'Nimble 9B weights', url: 'https://huggingface.co/bespokelabs/Bespoke-Nimble-9B' }],
    detail: 'The upstream MLX scorer needs merged, unquantized weights. Our NVIDIA test used NF4 weights. That result does not establish full-precision accuracy.',
  },
}

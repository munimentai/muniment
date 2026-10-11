// The first run asks one question: which model to connect. The answer list
// comes from the provider scan: local servers it found on this machine, and
// hints that the user already holds a subscription. Found entries lead, and
// "More providers" always ends the list.

// The subscriptions Muniment signs in to, in their default order. `catalog`
// is the provider-catalog id that Settings → Model providers opens, and
// `logo` is the full-color mark.
export const SUBSCRIPTIONS = [
  { id: 'anthropic', name: 'Claude', plan: 'Pro or Max, through Claude Code', logo: 'anthropic', catalog: 'anthropic' },
  { id: 'openai', name: 'ChatGPT', plan: 'Plus or Pro', logo: 'openai', catalog: 'openai' },
  { id: 'xai', name: 'Grok', plan: 'Grok Build', logo: 'xai', catalog: 'xai' },
  { id: 'google', name: 'Google', plan: 'Antigravity', logo: 'antigravity-color', catalog: 'google' },
  { id: 'github-copilot', name: 'GitHub Copilot', plan: 'Copilot plan', logo: 'github-copilot', catalog: 'github-copilot' },
  { id: 'meta', name: 'Meta Muse', plan: 'Muse Code', logo: 'meta', catalog: 'meta' },
  { id: 'kimi', name: 'Kimi', plan: 'Kimi Code', logo: 'kimi-coding', catalog: 'kimi' },
  { id: 'devin', name: 'Devin', plan: 'Devin plan', logo: 'devin', catalog: 'devin' },
  { id: 'openrouter', name: 'OpenRouter', plan: 'Credits', logo: 'openrouter', catalog: 'openrouter' },
]

// A local server is ready, needs a key, or is installed and not serving.
const SERVER_RANK = { ready: 0, 'needs-key': 2, 'not-running': 3 }

// Builds the ordered connect list: ready servers, hinted subscriptions,
// servers that need a step, then the other subscriptions in catalog order.
export function connectOptions(scan) {
  const hints = new Map((scan?.hints ?? []).map((hint) => [hint.provider, hint]))
  const servers = (scan?.servers ?? []).map((server) => ({
    kind: 'server',
    id: server.id,
    name: server.name,
    logo: server.id,
    server,
    rank: SERVER_RANK[server.state] ?? 3,
  }))
  const subscriptions = SUBSCRIPTIONS.map((entry, index) => ({
    kind: 'subscription',
    ...entry,
    hint: hints.get(entry.id) ?? null,
    rank: hints.has(entry.id) ? 1 : 4,
    index,
  }))
  return [...servers, ...subscriptions]
    .map((option, order) => ({ option, order }))
    .sort((a, b) => a.option.rank - b.option.rank || a.order - b.order)
    .map(({ option }) => option)
}

// The line under a server's name: where it answers and what it holds.
export function serverDetail(server) {
  const address = server.baseUrl?.replace(/^https?:\/\//, '').replace(/\/v1\/?$/, '')
  if (server.state === 'not-running') return 'Installed'
  const models = server.modelCount === 1 ? '1 model' : `${server.modelCount} models`
  return [address, server.modelCount ? models : null].filter(Boolean).join(' · ')
}

export const SERVER_STATES = {
  ready: { label: 'Ready', action: 'Connect', tone: 'signal' },
  'needs-key': { label: 'Needs key', action: 'Add key', tone: 'ochre' },
  'not-running': { label: 'Not running', action: 'Start', tone: 'muted' },
}

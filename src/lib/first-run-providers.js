import { registry } from './model-registry.js'

// The first run asks one question: which model to connect. The answer list
// comes from the provider scan: local servers it found on this machine, and
// hints that the user already holds a subscription. Found entries lead, and
// "More providers" always ends the list.

// The subscriptions Muniment signs in to, in their default order, from the
// model registry. `catalog` is the provider-catalog id that Settings → Model
// providers opens, and `logo` is the full-color mark.
export const SUBSCRIPTIONS = registry.subscriptions

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

export const SERVER_STATES = {
  ready: { action: 'Connect', tone: 'signal' },
  'needs-key': { action: 'Add key', tone: 'ochre' },
  'not-running': { action: 'Start', tone: 'muted' },
}

// How long the first run waits before it scans again. At first launch the
// runtime is still starting, so the first scans can fail before one answers.
export const SCAN_RETRY_DELAYS = [1000, 2000, 3000, 5000, 8000]

// Runs the provider scan, and runs it again after each delay while it fails.
// The last failure throws. `wait` is the timer, replaceable in tests.
export async function scanProviders(invoke, delays = SCAN_RETRY_DELAYS, wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))) {
  for (let attempt = 0; ; attempt += 1) {
    try {
      return await invoke('onboarding_provider_scan')
    } catch (error) {
      if (attempt >= delays.length) throw error
      await wait(delays[attempt])
    }
  }
}

// The servers a scan found that no connected provider serves yet, matched by
// address, for Settings → Connect account.
export function foundServers(scan, inventory) {
  const address = (url) => String(url ?? '').replace(/\/+$/, '').toLowerCase()
  const connected = new Set((inventory?.providers ?? []).map((provider) => address(provider.base_url)).filter(Boolean))
  return (scan?.servers ?? []).filter((server) => !connected.has(address(server.baseUrl)))
}

// What one click on a found server does, as its row names it.
export function foundServerAction(server) {
  if (server.state === 'ready') return 'Connect'
  if (server.state === 'needs-key') return server.keyFile ? `Connect with the key in ${server.keyFile}` : 'Add key'
  return 'Not running'
}

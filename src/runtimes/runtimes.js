// The runtimes this app attaches to: labels, states and the Settings pages
// that change while a paired runtime is selected. `runtime_list` answers
// { appVersion, deviceName, selected, runtimes }, this computer first.

export const LOCAL_RUNTIME_ID = 'local'
export const DEFAULT_SSH_COMMAND = 'muniment-server'
export const DEFAULT_SSH_PORT = 7443

const KINDS = {
  local: ['This computer', 'monitor'],
  desktop: ['Desktop', 'laptop'],
  server: ['Server', 'server'],
  container: ['Container', 'container'],
}

export function kindLabel(runtime) {
  return KINDS[runtime?.kind]?.[0] ?? 'Runtime'
}

export function kindIcon(runtime) {
  return KINDS[runtime?.kind]?.[1] ?? 'server'
}

// [label, next step, tone]. The tone picks ink, ochre for caution or oxide for a refusal.
const STATES = {
  unknown: ['Not checked', '', 'quiet'],
  connecting: ['Connecting', '', 'quiet'],
  online: ['Online', '', 'ink'],
  offline: ['Offline', 'Check that it is running and reachable.', 'caution'],
  certificate_changed: ['Certificate changed', 'Pair it again to trust its new certificate.', 'critical'],
  revoked: ['Access revoked', 'Pair it again with a new link.', 'critical'],
  version_mismatch: ['Version mismatch', 'Update the app or the runtime.', 'caution'],
  runtime_mismatch: ['Different runtime', 'Another runtime answers at its address. Pair it again.', 'critical'],
}

export function statusLabel(runtime) {
  return STATES[runtime?.status]?.[0] ?? 'Not checked'
}

export function statusHint(runtime) {
  return STATES[runtime?.status]?.[1] ?? ''
}

export function statusTone(runtime) {
  return STATES[runtime?.status]?.[2] ?? 'quiet'
}

export function activeRuntime(list) {
  return list?.runtimes?.find((runtime) => runtime.active) ?? list?.runtimes?.[0] ?? null
}

export function remoteSelected(list) {
  return !!list && list.selected !== LOCAL_RUNTIME_ID
}

export function pairedRuntimes(list) {
  return (list?.runtimes ?? []).filter((runtime) => !runtime.local)
}

export function updatableRuntimes(list) {
  return pairedRuntimes(list).filter((runtime) => runtime.updateOffered)
}

// The version a row shows: mono, with the leading v.
export function versionLabel(runtime) {
  if (!runtime?.version) return ''
  return runtime.version.startsWith('v') ? runtime.version : `v${runtime.version}`
}

// Settings pages a paired runtime refuses: the cloud account and phone pairing
// belong to this computer. They hide while a paired runtime is selected.
export const REMOTE_HIDDEN_PAGES = ['account']

// Settings pages that keep this computer's data while a paired runtime is
// selected. Each says so in one line.
export const LOCAL_PAGES = ['mcp', 'skills', 'plugins', 'home']

// One sentence from a command failure, ending in a period.
export function errorLine(failure, fallback = 'The runtime did not answer.') {
  const text = typeof failure === 'string' ? failure : failure?.message ?? String(failure ?? '')
  const line = text.trim().split('\n')[0]?.trim() || fallback
  return /[.!?]$/.test(line) ? line : `${line}.`
}

// A pasted link that looks like a pairing link. The runtime checks the rest.
export function looksLikePairingLink(text) {
  return /^muniment:\/\/pair\?/i.test(String(text ?? '').trim())
}

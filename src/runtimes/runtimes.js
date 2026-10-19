// The runtimes this app attaches to: labels, states and the Settings pages
// that change while a paired runtime is selected. `runtime_list` answers
// { appVersion, deviceName, selected, runtimes }, this computer first.

export const LOCAL_RUNTIME_ID = 'local'
export const DEFAULT_SSH_COMMAND = 'muniment-server'
export const DEFAULT_SSH_PORT = 7443

const KINDS = {
  local: ['This computer', 'monitor'],
  desktop: ['Desktop', 'laptop'],
  'desktop-client': ['Desktop', 'laptop'],
  server: ['Server', 'terminal'],
  container: ['Container', 'container'],
}

// The icon for the shape a machine announces. A server draws a rack only on
// rack hardware, and a terminal on any other host.
const SHAPES = { laptop: 'laptop', all_in_one: 'monitor', desktop: 'pc-case', server: 'server' }

export function kindLabel(runtime) {
  return KINDS[runtime?.kind]?.[0] ?? 'Runtime'
}

// The name core gives a runtime that announces no name of its own.
const GENERIC_NAMES = new Set(['Muniment runtime'])

// The one name the app shows for a machine: This computer for the local one,
// else the machine's own name, or its kind while it announces only the
// generic runtime name.
export function machineName(runtime) {
  if (!runtime) return 'Machine'
  if (runtime.local || runtime.id === LOCAL_RUNTIME_ID) return 'This computer'
  const name = String(runtime.name ?? '').trim()
  return name && !GENERIC_NAMES.has(name) ? name : kindLabel(runtime)
}

export function kindIcon(runtime) {
  const kind = runtime?.kind
  if (kind === 'container') return 'container'
  if (kind === 'server') return runtime?.formFactor === 'server' ? 'server' : 'terminal'
  return SHAPES[runtime?.formFactor] ?? KINDS[kind]?.[1] ?? 'server'
}

// [label, next step, tone]. The tone picks ink, ochre for caution or oxide for a refusal.
const STATES = {
  unknown: ['Not checked', '', 'quiet'],
  connecting: ['Connecting', '', 'quiet'],
  online: ['Online', '', 'ink'],
  offline: ['Offline', 'Check that it is running and reachable.', 'caution'],
  certificate_changed: ['Certificate changed', 'Add it again to trust its new certificate.', 'critical'],
  revoked: ['Access revoked', 'Add it again with a new code.', 'critical'],
  version_mismatch: ['Version mismatch', 'Update the app or the runtime.', 'caution'],
  runtime_mismatch: ['Different runtime', 'Another runtime answers at its address. Add it again.', 'critical'],
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
  return withV(runtime?.version)
}

function withV(version) {
  if (!version) return ''
  return version.startsWith('v') ? version : `v${version}`
}

// Whether version `a` is older than `b`, as semver orders them, or null when
// either is not a version.
export function olderVersion(a, b) {
  const parse = (version) => String(version ?? '').trim().replace(/^v/, '').match(/^(\d+)\.(\d+)\.(\d+)(-[0-9A-Za-z.-]+)?(?:\+.*)?$/)
  const left = parse(a)
  const right = parse(b)
  if (!left || !right) return null
  for (const index of [1, 2, 3]) {
    if (Number(left[index]) !== Number(right[index])) return Number(left[index]) < Number(right[index])
  }
  if (!left[4] !== !right[4]) return !!left[4]
  return (left[4] ?? '') < (right[4] ?? '')
}

// The work a draining runtime waits for, in plain words.
const BLOCKERS = {
  active_run: 'a reply in progress',
  pending_permission_gate: 'a permission request',
  authentication_operation: 'a sign-in',
  session_refresh: 'a session refresh',
  in_flight_external_effect: 'an action in progress',
}

export function blockersLine(blockers = []) {
  const words = blockers.map((blocker) => BLOCKERS[blocker] ?? 'other work')
  if (!words.length) return ''
  const list = words.length === 1 ? words[0] : `${words.slice(0, -1).join(', ')} and ${words.at(-1)}`
  return `Waiting for ${list}.`
}

const PENDING = ['queued', 'downloading', 'draining', 'installing', 'restarting']

// The update line of a machine row, from the state the runtime reports:
// { text, tooltip } or null for a machine that shows only its version. The
// app never says a machine updates unless the machine says so.
export function updateLabel(runtime) {
  const status = runtime?.updateStatus
  const target = status?.targetVersion
  // A status for a version the machine already runs is over.
  const reached = !!target && olderVersion(runtime?.version, target) === false
  if (status && !reached) {
    switch (status.state) {
      case 'queued': return { text: 'Update queued', tooltip: '' }
      case 'downloading': return { text: target ? `Downloading ${withV(target)}` : 'Downloading update', tooltip: '' }
      case 'draining': return { text: 'Finishing current work', tooltip: blockersLine(status.blockers) }
      case 'installing':
      case 'restarting': return { text: target ? `Updating to ${withV(target)}` : 'Updating', tooltip: '' }
      case 'failed': return { text: 'Update failed', tooltip: status.error ?? '' }
    }
  }
  if (runtime?.updateRequested) return { text: 'Update requested', tooltip: '' }
  if (runtime?.behind && runtime?.updateCapability === 'manual' && runtime?.updateTarget) {
    return { text: `${withV(runtime.updateTarget)} available, update manually`, tooltip: '' }
  }
  return null
}

// Whether the row menu offers Update: the machine is behind, answers, takes
// requests from this device and installs releases itself, and no update for
// it is on the way.
export function canRequestUpdate(runtime) {
  return !runtime?.local && !!runtime?.updateOffered && runtime?.status === 'online'
    && !PENDING.includes(runtime?.updateStatus?.state)
}

// Why Update all left a machine alone.
const SKIPS = {
  offline: 'offline',
  no_permission: 'no permission',
  manual: 'manual install',
  current: 'already current',
}

export function skipLabel(skip) {
  return `${skip.name} (${SKIPS[skip.reason] ?? skip.reason})`
}

// The parts of Settings pages that keep this computer's data while a paired
// runtime is selected. Each says so in one line.
export const LOCAL_PARTS = ['mcp', 'skill', 'plugin', 'retention', 'storage', 'companies']

// Settings pages that read and change the selected machine, in whole or in
// part. Once another machine answers, each names its scope at its top:
// All machines, or one machine.
export const MACHINE_PAGES = ['account', 'models', 'tools', 'permissions', 'data']

// The scope that changes every machine at once. Fleet sync carries each
// shared setting from the machine that saves it to every other one.
export const ALL_MACHINES = 'all'

// The machine scope a Settings page shows: the selected machine and the
// machines it can switch to, or null for a page without a choice or while
// no other machine answers.
export function settingsMachine(list, page) {
  if (!MACHINE_PAGES.includes(page)) return null
  const selected = activeRuntime(list)
  const choices = (list?.runtimes ?? []).filter((runtime) => runtime.local || runtime.status === 'online' || runtime.id === selected?.id)
  return selected && choices.some((runtime) => !runtime.local) ? { selected, choices } : null
}

// The line a part shows while Settings changes all machines and the part
// stays on each machine. `machine` names the machine the change reaches.
export function allMachinesNote(part, machine = 'this computer') {
  if (['mcp', 'skill', 'plugin'].includes(part)) return 'What you add reaches every machine. On or off changes only this computer.'
  if (part === 'models') return 'Sign-ins stay on the machine that holds them. Machines can share them with the others.'
  if (['context', 'storage', 'companies'].includes(part)) return `This part changes only ${['storage', 'companies'].includes(part) ? 'this computer' : machine}. Each machine keeps its own.`
  return ''
}

// One sentence from a command failure, ending in a period.
export function errorLine(failure, fallback = 'The runtime did not answer.') {
  const text = typeof failure === 'string' ? failure : failure?.message ?? String(failure ?? '')
  const line = text.trim().split('\n')[0]?.trim() || fallback
  return /[.!?]$/.test(line) ? line : `${line}.`
}

// A failure's whole text, which a failure note shows as its first line with
// the rest behind Show log.
export function errorText(failure, fallback = 'The runtime did not answer.') {
  const text = typeof failure === 'string' ? failure : failure?.message ?? String(failure ?? '')
  return text.trim() || fallback
}

// A pasted link that looks like a pairing link. The runtime checks the rest.
export function looksLikePairingLink(text) {
  return /^muniment:\/\/pair\?/i.test(String(text ?? '').trim())
}

// Where an address a pairing link names comes from.
const ENDPOINT_KINDS = {
  configured: 'Set in these settings',
  loopback: 'This computer only',
  lan: 'Local network',
  overlay: 'Private overlay network',
  public: 'Public address',
}

export function endpointKindLabel(kind) {
  return ENDPOINT_KINDS[kind] ?? 'Address'
}

// The port the settings name, else the port the listener bound.
export function listenPort(access) {
  const address = access?.settings?.listen ?? access?.listening
  const port = Number(String(address ?? '').split(':').pop())
  return Number.isInteger(port) && port > 0 ? port : null
}

export function seenLabel(value, now = Date.now()) {
  const date = value ? new Date(value) : null
  if (!date || Number.isNaN(date.getTime())) return 'Never seen'
  const minutes = Math.round((now - date.getTime()) / 60000)
  if (minutes < 2) return 'Now'
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return date.toLocaleDateString()
}

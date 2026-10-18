// A fleet for the probe: this laptop, a Studio desktop that is away and a
// Pod container that answers. This laptop reaches neither directly; it sees
// their threads, agents and projects through the swarm. `fleetSyncAnswer`
// answers the fleet operations with the shapes the core protocol fixtures
// show. `?state=` picks synced, syncing, offline, bootstrap, owners, queued,
// replica, offload, offline-settings, log or projects.

export const LAPTOP = '018f0000-0000-7000-8000-000000000041'
export const STUDIO = '018f0000-0000-7000-8000-000000000042'
export const POD = '018f0000-0000-7000-8000-000000000046'
const EDITOR = '018f0000-0000-4000-8000-0000000000a3'
export const STUDIO_THREAD = 'fleet-studio-memo'
export const POD_THREAD = 'fleet-pod-sweep'
const at = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString()

export const fleetSyncState = new URLSearchParams(location.search).get('state') ?? 'synced'

// The thread a state opens comes first, as the newest.
export function fleetSyncSummaries() {
  const open = fleetSyncThread()
  return [
    { threadId: 'fleet-local', title: 'Lease renewal', updatedAt: at(5), runtimeId: 'local', owner_runtime: LAPTOP },
    { threadId: STUDIO_THREAD, title: 'Draft board memo', updatedAt: at(40), runtimeId: 'local', owner_runtime: STUDIO, replica: true },
    { threadId: POD_THREAD, title: 'Nightly log sweep', updatedAt: at(90), runtimeId: 'local', owner_runtime: POD, replica: true },
    { threadId: 'fleet-launch', title: 'Launch checklist', updatedAt: at(300), runtimeId: 'local', owner_runtime: STUDIO, replica: true },
  ].map((summary) => summary.threadId === open ? { ...summary, updatedAt: at(1) } : summary)
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
}

export const fleetSyncProjects = {
  projects: { 'project-lease': 'Lease renewal' },
  remote: {
    'project-launch': { name: 'Launch plan', ownerRuntime: STUDIO, path: '/Users/alice/Documents/muniment/projects/Launch plan' },
    'project-sweeps': { name: 'Log sweeps', ownerRuntime: POD, path: '/home/muniment/projects/Log sweeps' },
  },
  threads: { 'fleet-local': 'project-lease', 'fleet-launch': 'project-launch' },
}

// The Studio thread's history is here; the Pod thread's is on its way.
export function fleetSyncHistory() {
  if (fleetSyncState !== 'queued') return []
  return [{
    runId: 'fleet-memo-run',
    prompt: 'Draft the board memo from the Q3 notes.',
    phase: 'complete',
    text: 'The memo leads with revenue up 12 percent, then the two hires and the office move.',
    receipt: { time: '4.1s' },
    toolActivity: [],
    resumable: false,
  }]
}

// The thread the state opens, if any.
export function fleetSyncThread() {
  if (fleetSyncState === 'queued') return STUDIO_THREAD
  if (fleetSyncState === 'replica') return POD_THREAD
  return null
}

const member = (runtime_id, runtime_name, form_factor, online, self = false) => ({ runtime_id, runtime_name, form_factor, self, online, stale: false, heartbeat_at: online ? at(0) : at(60 * 26), last_client_at: at(30), links: [], link: null, fleet: { trusted: true, role: 'full', removed: false } })

const classes = {
  synced: [['agent', 3, 3], ['group', 0, 0], ['thread', 4, 4], ['history', 48, 48]],
  bootstrap: [['agent', 3, 3], ['group', 0, 0], ['thread', 412, 412], ['project', 2, 2], ['history', 3120, 9800], ['artifact', 14, 61], ['extension', 0, 9]],
}

function syncStatus() {
  const state = { syncing: 'synced', bootstrap: 'bootstrapping', offline: 'offline' }[fleetSyncState] ?? 'synced'
  const rows = (fleetSyncState === 'bootstrap' ? classes.bootstrap : classes.synced).map(([name, held, total]) => ({ class: name, held, total, percent: total ? Math.floor((held / total) * 100) : 100 }))
  const synced = new Date()
  synced.setHours(14, 5, 0, 0)
  return { state, classes: rows, last_sync_at: synced.toISOString(), snapshot_needed_at: null, bootstrapped_at: state === 'bootstrapping' ? null : at(60 * 24 * 3), offline: { ...offline } }
}

const offline = { thread_days: 30, attachments: 'open' }
let pins = { pins: [{ class: 'artifact', key: 'report/index.html' }], offline: { artifacts: 'pinned', budget_bytes: 5368709120, prefetch: true }, usage: { fetched_bytes: 48_000_000, own_bytes: 12_000_000 } }

function op(op_id, threadId, ownerRuntime, ownerName, state = 'queued', minutes = 2) {
  return {
    op_id,
    operation: 'thread.post',
    item: { kind: 'thread', id: threadId },
    owner_runtime: ownerRuntime,
    owner_name: ownerName,
    state,
    reason: state === 'queued' ? { code: 'owner_offline', text: `${ownerName} is offline.`, last_seen_at: at(60 * 26) } : null,
    result: null,
    error: null,
    created_at: at(minutes),
    updated_at: at(minutes),
    expires_at: at(-60 * 24 * 7),
  }
}

let ops = fleetSyncState === 'queued'
  ? [op('fleet-op-1', STUDIO_THREAD, STUDIO, 'Studio')]
  : fleetSyncState === 'syncing'
    ? [op('fleet-op-1', STUDIO_THREAD, STUDIO, 'Studio'), op('fleet-op-2', POD_THREAD, POD, 'Pod', 'sent', 1)]
    : []

const credentials = () => ({
  share_models: true,
  role: 'full',
  removed: false,
  secrets: [],
  audit: [],
  accounts: fleetSyncState === 'bootstrap'
    ? [
        { name: 'router:work', provider: 'openai-codex', family: 'openai', kind: 'subscription', label: 'Work', holder: POD, holder_name: 'Pod', shared: true, this_machine: false, usable_here: false },
        { name: 'pi:anthropic', provider: 'anthropic', family: 'anthropic', kind: 'subscription', label: 'Claude', holder: STUDIO, holder_name: 'Studio', shared: true, this_machine: false, usable_here: false },
      ]
    : [],
})

// `fleet.audit` entries as core answers them: keyed `<writer>:<uuidv7>`,
// with the writer as `actor_runtime`. The owner of a forwarded action writes
// it, and its `runtime_id` names the sender.
const auditKey = (writer, n) => `${writer}:019a0000-0000-7000-8000-0000000000${String(n).padStart(2, '0')}`
const audit = [
  { id: auditKey(LAPTOP, 1), at: at(3), kind: 'op', actor_runtime: LAPTOP, runtime_id: POD, record: { op_id: '019a0000-0000-7000-8000-0000000000f1', operation: 'thread.post', actor_device: 'local', origin: POD, result: 'applied', note: null } },
  { id: auditKey(LAPTOP, 2), at: at(25), kind: 'sealed', actor_runtime: LAPTOP, runtime_id: null, record: { secret_id: `models:${LAPTOP}`, version: 4, recipients: [STUDIO, POD], keys: ['router:team', 'pi:anthropic'] } },
  { id: auditKey(POD, 3), at: at(26), kind: 'opened', actor_runtime: POD, runtime_id: LAPTOP, record: { secret_id: `models:${LAPTOP}`, version: 4, recipients: [STUDIO, POD], keys: ['router:team', 'pi:anthropic'] } },
  { id: auditKey(STUDIO, 4), at: at(60 * 5), kind: 'hold_moved', actor_runtime: STUDIO, runtime_id: LAPTOP, record: { account: 'pi:anthropic' } },
  { id: auditKey(LAPTOP, 5), at: at(60 * 26), kind: 'role_changed', actor_runtime: LAPTOP, runtime_id: POD, record: { from: 'worker', to: 'full' } },
  { id: auditKey(LAPTOP, 6), at: at(60 * 49), kind: 'snapshot_served', actor_runtime: LAPTOP, runtime_id: POD, record: { rows: 1840, complete: true } },
  { id: auditKey(LAPTOP, 7), at: at(60 * 72), kind: 'vault_token_issued', actor_runtime: LAPTOP, runtime_id: STUDIO, record: { pairing_id: 'pairing-studio' } },
]

let historyPolls = 0

// The answer to one `agent_request`, or undefined to fall through to the
// agents fixture.
export function fleetSyncAnswer({ operation, body = {} }) {
  if (operation === 'swarm.members') return { self: LAPTOP, members: [member(LAPTOP, 'Laptop', 'laptop', true, true), member(STUDIO, 'Studio', 'desktop', false), member(POD, 'Pod', 'container', true)], absence: { paused: false, paused_at: null, last_client_at: at(1) } }
  if (operation === 'swarm.agents') return { agents: [{ agent_id: EDITOR, name: 'Release editor', label: 'Writes release notes', avatar: null, primary_runtime: STUDIO, primary_thread: STUDIO_THREAD, epoch: 1, updated_at: at(60), activity: null, move_suggested: true }] }
  if (operation === 'group.list') return { groups: [] }
  if (operation === 'agent.approvals') return { approvals: [] }
  if (operation === 'agent.list') return { agents: [], primary_threads: {}, latest_runs: {}, errors: [] }
  if (operation === 'fleet.sync_status') return syncStatus()
  if (operation === 'fleet.ops') return { ops: structuredClone(ops.filter((entry) => !body.state || body.state !== 'open' || ['queued', 'sent'].includes(entry.state))) }
  if (operation === 'fleet.op_status') return { op: structuredClone(ops.find((entry) => entry.op_id === body.op_id)) }
  if (operation === 'fleet.op_cancel') {
    ops = ops.map((entry) => entry.op_id === body.op_id ? { ...entry, state: 'cancelled', reason: null } : entry)
    return { op: structuredClone(ops.find((entry) => entry.op_id === body.op_id)) }
  }
  if (operation === 'fleet.credentials') return credentials()
  if (operation === 'fleet.pins') return structuredClone(pins)
  if (operation === 'fleet.pin_set') {
    if (body.offline) pins = { ...pins, offline: { ...pins.offline, ...body.offline } }
    for (const pin of body.pins ?? []) pins = { ...pins, pins: [...pins.pins.filter((entry) => entry.class !== pin.class || entry.key !== pin.key), ...(pin.pinned ? [{ class: pin.class, key: pin.key }] : [])] }
    return structuredClone(pins)
  }
  if (operation === 'fleet.offline_set') { Object.assign(offline, body); return { offline: { ...offline } } }
  if (operation === 'fleet.audit') return { entries: structuredClone(audit).slice(0, body.limit ?? 100), more: audit.length > (body.limit ?? 100) }
  if (operation === 'thread.history_fetch') {
    // The Pod thread's history is still on its way; the Studio thread is here.
    if (body.thread_id === POD_THREAD) { historyPolls += 1; return { thread_id: POD_THREAD, owner_runtime: POD, status: 'fetching', held_events: 12, total_events: 48, before: null } }
    return { thread_id: body.thread_id, owner_runtime: STUDIO, status: 'held', held_events: 48, total_events: 48, before: null }
  }
  if (operation === 'preferences.get' || operation === 'preferences.import') throw 'The runtime does not know this operation.'
  return undefined
}

let offloadStep = 0
let projectsOpened = false

// Moves the page to the state's screen, one step per render.
export function advanceFleetSync() {
  const state = fleetSyncState
  if (state === 'owners' && !projectsOpened) {
    const launch = [...document.querySelectorAll('.project-title')].find((button) => button.textContent.includes('Launch plan'))
    if (!launch) return
    projectsOpened = true
    launch.click()
    return
  }
  if (state === 'offload') {
    // A new thread takes a machine; an open one keeps its own.
    if (!document.querySelector('[data-fresh-thread]')) return document.querySelector('.side-action.new-thread:not(:disabled)')?.click()
    const chip = document.querySelector('.machine-picker button[aria-expanded]')
    if (!chip) return
    if (offloadStep === 0) { offloadStep = 1; chip.click(); return }
    const pod = [...document.querySelectorAll('.machine-menu [role="menuitemradio"]')].find((item) => item.textContent.includes('Pod'))
    if (offloadStep === 1 && pod) { offloadStep = 2; pod.click(); return }
    if (offloadStep === 2 && chip.textContent.includes('Pod')) {
      const composer = document.getElementById('composer-message')
      composer.value = 'Sweep last night\'s container logs for errors.'
      composer.dispatchEvent(new Event('input', { bubbles: true }))
      offloadStep = 3
      chip.click()
      return
    }
    return
  }
  if (state === 'projects') {
    if (document.querySelector('[aria-label="Projects catalog"]')) return
    const search = document.querySelector('#sidebar input[aria-label="Search threads"]')
    if (search && search.value !== 'Projects') { search.value = 'Projects'; search.dispatchEvent(new Event('input', { bubbles: true })); return }
    document.querySelector('#sidebar [aria-label="Search results"] button[aria-label="Projects"]')?.click()
    return
  }
  if (state === 'offline-settings' || state === 'log') {
    if (!document.querySelector('.settings-layout')) return document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
    const machines = [...document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button')].find((button) => button.textContent.trim() === 'Machines')
    if (machines && machines.getAttribute('aria-current') !== 'true') return machines.click()
    const details = document.querySelector('.settings-runtimes [aria-label="Settings for This computer"][aria-expanded="false"]')
    if (details) return details.click()
    const target = state === 'log' ? document.querySelector('[aria-label="Machine log"]') : [...document.querySelectorAll('.settings-heading')].find((heading) => heading.textContent.startsWith('Available offline'))
    target?.closest('section')?.scrollIntoView({ block: 'start' })
  }
}

// Whether the state's screen is ready for a capture.
export function fleetSyncRendered() {
  const state = fleetSyncState
  const line = document.querySelector('.sync-line')?.textContent ?? ''
  if (state === 'synced') return line.includes('Synced')
  if (state === 'syncing') return line.includes('Syncing 2')
  if (state === 'offline') return line.includes('Offline · synced 14:05')
  if (state === 'bootstrap') return !!document.querySelector('[aria-label="Sync progress"]') && document.body.textContent.includes('arriving from Pod')
  if (state === 'owners') return document.querySelectorAll('#sidebar .owner-chip').length >= 3 && !!document.querySelector('.project-threads') && document.body.textContent.includes('Release editor')
  if (state === 'queued') return document.body.textContent.includes("Queued. Studio is offline. Sends when it's back.") && document.body.textContent.includes('Move here')
  if (state === 'replica') return document.body.textContent.includes('History arriving from Pod: 12 of 48 events.')
  if (state === 'offload') return offloadStep === 3 && !!document.querySelector('.machine-menu [role="menuitemradio"][aria-checked="true"]')?.textContent.includes('Pod')
  if (state === 'offline-settings') return document.body.textContent.includes('Details only. Files stay on the machine that owns the project.') && !document.querySelector('[role="radiogroup"][aria-label="Projects"]')
  if (state === 'log') return (document.querySelector('[aria-label="Machine log"]')?.children.length ?? 0) === audit.length && document.body.textContent.includes('Pod → Laptop')
  if (state === 'projects') {
    const catalog = document.querySelector('[aria-label="Projects catalog"]')?.textContent ?? ''
    return catalog.includes('Lease renewal') && catalog.includes('files on Studio') && catalog.includes('files on Pod')
  }
  return false
}

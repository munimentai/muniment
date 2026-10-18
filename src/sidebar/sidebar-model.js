// What the sidebar hands core and keeps on this device. Core's
// `sidebar::arrange` places every thread in one project and in one section,
// with its status in words. This module gathers what the window knows for it,
// keeps when the user last read each thread, and names the tone of a status.

export const CHATS_PROJECT = 'chats'
export const READ_KEY = 'muniment.thread-read'
export const READ_BASELINE_KEY = 'muniment.thread-read-baseline'

// When the user last read each thread, and the time before which a thread
// with no read time counts as read. The baseline is set on the first read,
// so threads from before read times were kept start settled.
export function readMarks(storage = localStorage, now = new Date()) {
  let read = {}
  let baseline = null
  try {
    const stored = JSON.parse(storage.getItem(READ_KEY) || '{}')
    if (stored && typeof stored === 'object') read = Object.fromEntries(Object.entries(stored).filter(([, at]) => typeof at === 'string'))
    baseline = storage.getItem(READ_BASELINE_KEY)
    if (!baseline) {
      baseline = now.toISOString()
      storage.setItem(READ_BASELINE_KEY, baseline)
    }
  } catch (_) {
    baseline = baseline || now.toISOString()
  }
  return { read, baseline }
}

// The marks after reading `threadId` at `at`. Only the newest 2000 threads keep a mark.
export function markRead(marks, threadId, at = new Date(), storage = localStorage) {
  if (!threadId) return marks
  const read = { ...marks.read, [threadId]: at.toISOString() }
  const kept = Object.fromEntries(Object.entries(read).sort((a, b) => b[1].localeCompare(a[1])).slice(0, 2000))
  try { storage.setItem(READ_KEY, JSON.stringify(kept)) } catch (_) {}
  return { ...marks, read: kept }
}

const RUNNING = new Set(['queued', 'leased', 'running'])
const FAILED = new Set(['failed', 'interrupted'])

// The threads with a run in flight and the threads whose newest run failed,
// from the agent runs the window holds, newest first.
export function runStates(runs = []) {
  const working = new Set()
  const failed = new Set()
  const seen = new Set()
  for (const run of runs) {
    if (!run?.thread_id || seen.has(run.thread_id)) continue
    seen.add(run.thread_id)
    if (RUNNING.has(run.status)) working.add(run.thread_id)
    else if (FAILED.has(run.status)) failed.add(run.thread_id)
  }
  return { working, failed }
}

// The items of the notifications page that point at a thread or an agent.
export function attention(needsYou = []) {
  return needsYou.filter((item) => item && item.kind !== 'offline').map((item) => ({
    kind: item.kind,
    thread_id: item.run?.thread_id ?? item.approval?.thread_id ?? null,
    agent_id: item.agent_id ?? item.approval?.agent_id ?? null,
  }))
}

// Core's input from the window's state.
export function sidebarInput({ summaries = [], agentListing = null, agents = [], groups = [], catalog = { projects: {}, threads: {} }, remote = [], organization = {}, marks = { read: {}, baseline: null }, working = [], failed = [], needsYou = [], held = new Map(), current = null, filter = 'all', query = '', machineOf = () => null }) {
  const threads = summaries.map((summary) => ({ id: summary.threadId, title: summary.title || '', updated_at: summary.updatedAt || '', machine: machineOf(summary.runtimeId) }))
  const known = new Set(threads.map((thread) => thread.id))
  for (const list of Object.values(agentListing?.threads ?? {})) {
    for (const thread of list) {
      if (!thread?.thread_id || known.has(thread.thread_id)) continue
      known.add(thread.thread_id)
      threads.push({ id: thread.thread_id, title: thread.title || '', updated_at: thread.updated_at || '', machine: null })
    }
  }
  for (const group of groups) {
    if (!group?.thread_id) continue
    const thread = threads.find((entry) => entry.id === group.thread_id)
    if (thread) thread.title = group.name || thread.title
    else threads.push({ id: group.thread_id, title: group.name || '', updated_at: group.updated_at || '', machine: null })
  }
  const projects = [
    ...Object.entries(catalog.projects ?? {}).map(([id, name]) => ({ id, name, owner: null })),
    ...remote.map(([id, name, owner]) => ({ id, name, owner: owner?.name ?? null })),
  ]
  const agentThreads = {}
  for (const [agentId, list] of Object.entries(agentListing?.threads ?? {})) for (const thread of list) if (thread?.thread_id) agentThreads[thread.thread_id] = agentId
  for (const run of Object.values(agentListing?.latestRuns ?? {})) if (run?.thread_id && !agentThreads[run.thread_id]) agentThreads[run.thread_id] = run.agent_id
  const pinned = [], archived = []
  for (const [id, state] of Object.entries(organization)) {
    if (state?.pinned) pinned.push(id)
    if (state?.archived) archived.push(id)
  }
  return {
    threads,
    projects,
    assignments: { ...(catalog.threads ?? {}) },
    agents: agents.map((agent) => ({ id: agent.id, name: agent.name, project_id: agent.projectId ?? agent.project_id ?? null, home_thread_id: agentListing?.primaryThreads?.[agent.id] ?? null, paused_reason: agent.paused_reason ?? null, routines: (agent.routines ?? []).map((routine) => ({ enabled: routine.enabled !== false, proposed: routine.proposed === true, trigger: routine.trigger ?? null })) })),
    agent_threads: agentThreads,
    pinned,
    archived,
    read: marks.read,
    read_baseline: marks.baseline,
    working: [...working],
    failed: [...failed],
    needs_you: attention(needsYou),
    held: Object.fromEntries(held),
    current,
    filter,
    query,
  }
}

export function arrangeSidebar(tauri, input) {
  return tauri.invoke('sidebar_arrange', { input })
}

// Where each thread shows after an answer, so a read thread stays in place.
export function holdRows(held, sidebar) {
  for (const row of sidebar.recent) held.set(row.thread_id, 'recent')
  for (const row of sidebar.settled) held.set(row.thread_id, 'settled')
  return held
}

export function emptySidebar() {
  return { needs_you: [], working: [], pinned: [], projects: [], recent: [], settled: [], archived: [], results: [] }
}

// The tone of a row's status: needs you warns, working is active, a failure
// is danger, a reply waiting to be read is done, and a read thread is quiet.
const TONES = { needs_you: 'warning', working: 'active', failed: 'danger', done: 'success', read: 'neutral' }
export function statusTone(status) {
  return TONES[status] ?? 'neutral'
}

// Every row the sidebar shows, in its order and once each, for the row shortcuts.
export function shownRows(sidebar, { workingOpen = true, expanded = new Set(), settledOpen = false, fresh = false } = {}) {
  const seen = new Set()
  return [
    ...sidebar.needs_you,
    ...(workingOpen ? sidebar.working : []),
    ...sidebar.pinned,
    ...sidebar.projects.filter((project) => expanded.has(project.id)).flatMap((project) => project.threads),
    ...(fresh ? [{ thread_id: null }] : []),
    ...sidebar.recent,
    ...(settledOpen ? sidebar.settled : []),
  ].filter((row) => !seen.has(row.thread_id) && seen.add(row.thread_id))
}

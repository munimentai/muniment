// What the sidebar hands core and keeps on this device. Core's
// `sidebar::arrange` places every thread once in the Projects list, in its
// project or as a row of its own, with its status in words and, for an
// agent's thread, the agent's id, name and color. This module
// gathers what the window knows for it, keeps when the user last read each
// thread, and names the tone of a status.

import { agentColor } from '../lib/agent-avatar.js'

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

// The threads with a run in flight, with when that run started, and the
// threads whose newest run failed, from the agent runs the window holds,
// newest first.
export function runStates(runs = []) {
  const working = new Set()
  const failed = new Set()
  const since = new Map()
  const seen = new Set()
  for (const run of runs) {
    if (!run?.thread_id || seen.has(run.thread_id)) continue
    seen.add(run.thread_id)
    if (RUNNING.has(run.status)) {
      working.add(run.thread_id)
      if (run.created_at) since.set(run.thread_id, run.created_at)
    } else if (FAILED.has(run.status)) failed.add(run.thread_id)
  }
  return { working, failed, since }
}

// The items of the notifications page that point at a thread or an agent.
export function attention(needsYou = []) {
  return needsYou.filter((item) => item && item.kind !== 'offline').map((item) => ({
    kind: item.kind,
    thread_id: item.run?.thread_id ?? item.approval?.thread_id ?? null,
    agent_id: item.agent_id ?? item.approval?.agent_id ?? null,
  }))
}

// Core's input from the window's state. Each agent carries its color, which
// its rows show. `agentChats` are agent creation chats as
// { threadId, title, updatedAt, agentId }: a chat shows once it holds a
// message, and a saved agent's chat sits with the agent.
export function sidebarInput({ summaries = [], agentListing = null, agents = [], groups = [], agentChats = [], catalog = { projects: {}, threads: {} }, remote = [], organization = {}, marks = { read: {}, baseline: null }, working = [], failed = [], needsYou = [], current = null, query = '', machineOf = () => null }) {
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
  for (const chat of agentChats) {
    if (!chat?.threadId) continue
    const thread = threads.find((entry) => entry.id === chat.threadId)
    if (!thread) threads.push({ id: chat.threadId, title: chat.title || '', updated_at: chat.updatedAt || '', machine: null })
    else {
      if (chat.title) thread.title = chat.title
      if (Date.parse(chat.updatedAt ?? '') > (Date.parse(thread.updated_at) || 0)) thread.updated_at = chat.updatedAt
    }
  }
  const projects = [
    ...Object.entries(catalog.projects ?? {}).map(([id, name]) => ({ id, name, owner: null })),
    ...remote.map(([id, name, owner]) => ({ id, name, owner: owner?.name ?? null })),
  ]
  const agentThreads = {}
  for (const [agentId, list] of Object.entries(agentListing?.threads ?? {})) for (const thread of list) if (thread?.thread_id) agentThreads[thread.thread_id] = agentId
  for (const run of Object.values(agentListing?.latestRuns ?? {})) if (run?.thread_id && !agentThreads[run.thread_id]) agentThreads[run.thread_id] = run.agent_id
  for (const chat of agentChats) if (chat?.threadId && chat.agentId && !agentThreads[chat.threadId]) agentThreads[chat.threadId] = chat.agentId
  const pinned = [], archived = []
  for (const [id, state] of Object.entries(organization)) {
    if (state?.pinned) pinned.push(id)
    if (state?.archived) archived.push(id)
  }
  return {
    threads,
    projects,
    assignments: { ...(catalog.threads ?? {}) },
    agents: agents.map((agent) => ({ id: agent.id, name: agent.name, project_id: agent.projectId ?? agent.project_id ?? null, home_thread_id: agentListing?.primaryThreads?.[agent.id] ?? null, paused_reason: agent.paused_reason ?? null, routines: (agent.routines ?? []).map((routine) => ({ enabled: routine.enabled !== false, proposed: routine.proposed === true, trigger: routine.trigger ?? null })), color: agentColor(agent) })),
    agent_threads: agentThreads,
    pinned,
    archived,
    read: marks.read,
    read_baseline: marks.baseline,
    working: [...working],
    failed: [...failed],
    needs_you: attention(needsYou),
    current,
    query,
  }
}

export function arrangeSidebar(tauri, input) {
  return tauri.invoke('sidebar_arrange', { input })
}

export function emptySidebar() {
  return { projects: [], archived: [], results: [] }
}

// Where a new thread's row sits in the Projects list: after the pinned rows,
// as the newest activity.
export function freshIndex(sidebar) {
  const index = sidebar.projects.findIndex((project) => !project.pinned)
  return index < 0 ? sidebar.projects.length : index
}

// Every thread the sidebar arranged outside the archive, once each.
export function listedRows(sidebar) {
  return sidebar.projects.flatMap((project) => project.threads)
}

// The tone of a row's status: needs you warns, working is active, a failure
// is danger, a reply waiting to be read is done, and a read thread is quiet.
const TONES = { needs_you: 'warning', working: 'active', failed: 'danger', done: 'success', read: 'neutral' }
export function statusTone(status) {
  return TONES[status] ?? 'neutral'
}

// Every row the sidebar shows, in its order and once each, for the row
// shortcuts: a thread's own row, and the threads of each expanded project.
// A new thread's row has no id yet.
export function shownRows(sidebar, { expanded = new Set(), fresh = false } = {}) {
  const seen = new Set()
  const rows = sidebar.projects.map((project) => project.kind === 'thread' ? project.threads : expanded.has(project.id) ? project.threads : [])
  if (fresh) rows.splice(freshIndex(sidebar), 0, [{ thread_id: null }])
  return rows.flat().filter((row) => !seen.has(row.thread_id) && seen.add(row.thread_id))
}

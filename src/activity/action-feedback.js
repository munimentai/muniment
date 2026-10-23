import { compactionCounts } from '../lib/context-usage.js'
import { formatByteSize } from '../lib/chat-state.js'
import { callLine, rawInput } from './tool-call.js'

// The headings of a run of calls of one kind. Core names each call's kind.
const groupWords = {
  compact: { icon: 'combine', done: 'Context compacted', active: 'Compacting context' },
  read: { icon: 'book-open', done: 'Read files', active: 'Reading files' },
  search: { icon: 'search', done: 'Searched files', active: 'Searching files' },
  list: { icon: 'folder', done: 'Listed files', active: 'Listing files' },
  web: { icon: 'globe', done: 'Used the web', active: 'Using the web' },
  command: { icon: 'square-terminal', done: 'Ran commands', active: 'Running commands' },
  edit: { icon: 'square-pen', done: 'Changed files', active: 'Changing files' },
  agent: { icon: 'bot', done: 'Worked with agents', active: 'Working with agents' },
  memory: { icon: 'brain', done: 'Used memory', active: 'Using memory' },
  mcp: { icon: 'server', done: 'Called servers', active: 'Calling servers' },
  other: { icon: 'wrench', done: 'Used tools', active: 'Using tools' },
}

// One call as a row: its line, the file it may open, and whether it has
// details.
function actionOf(activity, live) {
  const line = callLine(activity, live)
  const { presentation } = line
  const kind = groupWords[presentation.kind] ? presentation.kind : 'other'
  const details = (presentation.details ?? []).length > 0
  return {
    ...activity,
    presentation,
    kind,
    calls: activity.uncounted ? 0 : 1,
    label: line.label,
    result: line.result,
    failed: line.failed,
    state: line.state,
    running: line.running,
    icon: presentation.icon,
    counts: kind === 'compact' && activity.status !== 'failed' ? compactionCounts(activity.output) : '',
    path: ['read', 'edit'].includes(kind) && typeof presentation.path === 'string' ? presentation.path : null,
    hasDetails: details || !!activity.output || !!activity.input,
    detailInput: rawInput(activity.input),
  }
}

// The rows of a run's calls. Core names the script that made a call in
// `parentEffectId`, gives the script its calls' kind and marks it
// `uncounted`, so the script is the parent row and its calls nest under it.
export function actionRows(activities = [], live = false) {
  const ids = new Set(activities.map((activity) => activity.effectId))
  const nested = (activity) => !!activity.parentEffectId && ids.has(activity.parentEffectId)
  const children = new Map()
  for (const activity of activities) {
    if (nested(activity)) children.set(activity.parentEffectId, [...(children.get(activity.parentEffectId) ?? []), activity])
  }
  const row = (activity) => {
    const action = actionOf(activity, live)
    const made = (children.get(activity.effectId) ?? []).map(row)
    if (!made.length) return action
    return { ...action, children: made, calls: action.calls + made.reduce((sum, child) => sum + child.calls, 0), running: action.running || made.some((child) => child.running) }
  }
  return activities.filter((activity) => !nested(activity)).map(row)
}

// Consecutive rows of one kind fold into a group, in order.
export function actionGroups(activities = [], live = false) {
  const groups = []
  for (const action of actionRows(activities, live)) {
    const { kind } = action
    let group = groups.at(-1)
    if (!group || group.kind !== kind) {
      group = { kind, id: action.effectId, ...groupWords[kind], actions: [], calls: 0 }
      groups.push(group)
    }
    group.actions.push(action)
    group.calls += action.calls
    group.running = group.running || action.running
    group.label = kind === 'compact' ? action.label : group.running ? group.active : group.done
  }
  return groups
}

export function actionDuration(start, end) {
  const seconds = Math.max(0, Math.floor((new Date(end) - new Date(start)) / 1000))
  if (!Number.isFinite(seconds)) return ''
  return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`
}

// A chat event carries the start of a long tool output. The detail reads the
// rest from the run's record, which answers a capped start of a very long one.
export function outputLimitNote(detail) {
  const shown = new TextEncoder().encode(detail?.text ?? '').length
  return `Showing the first ${formatByteSize(shown)} of ${formatByteSize(detail?.bytes)}.`
}

// Offsets count UTF-16 units, as JavaScript strings do. The journal derives them
// at tool start, so replay and streaming share the same order.
export function responseParts(run) {
  const text = run.text ?? ''
  const parts = []
  let cursor = 0
  for (const activity of run.toolActivity ?? []) {
    const offset = Math.max(cursor, Math.min(text.length, activity.textOffset ?? 0))
    if (offset > cursor) parts.push({ type: 'text', text: text.slice(cursor, offset) })
    if (parts.at(-1)?.type === 'actions') parts.at(-1).activities.push(activity)
    else parts.push({ type: 'actions', activities: [activity] })
    cursor = offset
  }
  if (!parts.length && !text && run.phase === 'streaming') parts.push({ type: 'text', text: '' })
  if (cursor < text.length) parts.push({ type: 'text', text: text.slice(cursor) })
  return parts
}

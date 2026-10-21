// The Notifications page's words. Core's `notifications.list` answers each
// machine's runs, the approvals waiting in the swarm and the next scheduled
// runs, and core's merge builds the page: what needs the user, what runs,
// and the rest by day. This module asks, names and formats.
import { agentRequest } from '../agents/agent-runtime.js'
import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'
import { routineIcon } from '../agents/routine-labels.js'
import { markdownPlainText } from '../lib/assistant-markdown.js'
import { formatClock, formatDate, sameDay, zoneOffsetMinutes } from '../lib/time-format.svelte.js'

export const NOTIFICATION_VIEWS = [
  { value: 'needs_you', label: 'Needs you' },
  { value: 'all', label: 'All' },
  { value: 'failed', label: 'Failed' },
]

export const READ_AT_KEY = 'notifications-read-at'
export const NOTIFY_KEY = 'notify'
// A system notification when a run needs the user is on by default, and
// when one finishes is off: a finished run waits on the page.
export const DEFAULT_NOTIFY = Object.freeze({ needs_you: true, finished: false })

export function readNotifications(tauri, machines, { view = 'all', machine = null, agent = null } = {}) {
  const filter = { view, utc_offset_minutes: zoneOffsetMinutes() }
  if (machine) filter.machine = machine
  if (agent) filter.agent = agent
  return tauri.invoke('notifications_list', { machines, filter })
}

// Marks everything read on every machine: the time is a fleet preference.
export function markAllRead(tauri, at = new Date()) {
  return agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.set', { values: { [READ_AT_KEY]: at.toISOString() } })
}

export function notifyPreferences(values) {
  const stored = values?.[NOTIFY_KEY]
  return {
    needs_you: typeof stored?.needs_you === 'boolean' ? stored.needs_you : DEFAULT_NOTIFY.needs_you,
    finished: typeof stored?.finished === 'boolean' ? stored.finished : DEFAULT_NOTIFY.finished,
  }
}

export async function readNotifyPreferences(tauri) {
  if (!tauri) return { ...DEFAULT_NOTIFY }
  try {
    const answer = await agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.get', {})
    return notifyPreferences(answer?.values)
  } catch (_) {
    return { ...DEFAULT_NOTIFY }
  }
}

export async function saveNotifyPreferences(tauri, preferences) {
  const value = { needs_you: !!preferences.needs_you, finished: !!preferences.finished }
  await agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.set', { values: { [NOTIFY_KEY]: value } })
  return value
}

// Whether a run's change raises a system notification under the user's
// choice: a question, a failure or a usage stop needs the user, a reply is
// finished.
export function notifyAllowed(run, preferences = DEFAULT_NOTIFY) {
  if (run?.status === 'completed') return !!preferences.finished
  return !!preferences.needs_you
}

const TRIGGERS = { cron: 'Schedule', webhook: 'Webhook', mcp: 'MCP', manual: 'Manual', agent_message: 'Agent' }
export function triggerLabel(trigger) {
  return TRIGGERS[trigger?.kind] ?? 'Event'
}

// The trigger chip's icon: a routine's own icon for a schedule, a webhook or
// a tool check, the Run now play for a manual run and a message for another
// agent's.
const TRIGGER_ICONS = { manual: 'play', agent_message: 'message-square' }
export function triggerIcon(trigger) {
  const kind = trigger?.kind
  if (kind in TRIGGER_ICONS) return TRIGGER_ICONS[kind]
  return ['cron', 'webhook', 'mcp'].includes(kind) ? routineIcon(trigger) : 'zap'
}

const STATUS = {
  running: { icon: 'loader-circle', label: 'Running' },
  done: { icon: 'circle-check', label: 'Done' },
  needs_you: { icon: 'circle-alert', label: 'Needs you' },
  failed: { icon: 'circle-x', label: 'Failed' },
  cancelled: { icon: 'circle-slash', label: 'Stopped' },
}
export function statusMark(status) {
  return STATUS[status] ?? STATUS.done
}

const DAYS = { today: 'Today', yesterday: 'Yesterday', earlier: 'Earlier' }
export function dayLabel(day) {
  return DAYS[day] ?? 'Earlier'
}

// The row's one line: core's summary of the result, the error, or the state,
// as plain text with the reply's Markdown syntax removed.
export function resultLine(entry) {
  const run = entry?.run ?? {}
  const summary = markdownPlainText(run.result_summary)
  if (entry?.status === 'failed') return run.error || 'The run failed.'
  if (entry?.status === 'needs_you') return summary || 'Waiting for your answer.'
  if (entry?.status === 'cancelled') return 'Stopped before it finished.'
  if (entry?.status === 'running') return 'Running now.'
  if (run.silent) return 'Nothing needed a reply.'
  return summary || 'Finished.'
}

const NEEDS = {
  approval: { icon: 'shield-alert', title: 'Approval needed' },
  input: { icon: 'message-square', title: 'Answer needed' },
  limit: { icon: 'triangle-alert', title: 'Usage limit reached' },
  offline: { icon: 'globe-off', title: 'Machine offline' },
}
export function needsMark(kind) {
  return NEEDS[kind] ?? NEEDS.input
}

// A row's time as the Time setting shows times: the clock time today, else the date.
export function clockTime(at, now = Date.now()) {
  const date = new Date(at)
  if (Number.isNaN(date.getTime())) return ''
  return sameDay(date, now) ? formatClock(date) : formatDate(date)
}

// When a scheduled run fires: the time today, else the day and the time.
export function scheduleTime(at, now = Date.now()) {
  const date = new Date(at)
  if (Number.isNaN(date.getTime())) return ''
  const time = formatClock(date)
  return sameDay(date, now) ? time : `${formatDate(date, { weekday: 'short', month: 'short', day: 'numeric' })}, ${time}`
}

// The run the page opens: its thread on the machine that answered for it.
export function runWithMachine(run, machine) {
  return { ...run, runtimeId: machine?.id || LOCAL_RUNTIME_ID }
}

export function isNotificationsShortcut(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  return event.key?.toLowerCase() === 'a'
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !!event.shiftKey
}

export function notificationsShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+Shift+A' : 'Control+Shift+A'
}

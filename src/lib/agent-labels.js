// The words the agent profile, the routines and Notifications show for a run and
// a trigger. The runtime decides every state, and these only name it.
import { executionTime } from './execution-time.js'

const runWords = {
  queued: 'Queued',
  leased: 'Starting',
  running: 'Running',
  waiting_input: 'Waiting for you',
  completed: 'Completed',
  failed: 'Failed',
  cancelled: 'Stopped',
  interrupted: 'Interrupted',
}
const sentence = (value) => value.charAt(0).toUpperCase() + value.slice(1).replaceAll('_', ' ')

export function runStatusLabel(status) {
  if (!status) return ''
  return runWords[status] || sentence(status)
}

// Runs that still hold the agent: Run now and Delete wait for them.
export function runActive(status) {
  return ['queued', 'leased', 'running', 'waiting_input'].includes(status)
}

// The tone a status reads in: danger for a failure, warning for a run that waits.
export function runTone(status) {
  return status === 'failed' ? 'critical' : status === 'waiting_input' || status === 'interrupted' ? 'caution' : 'quiet'
}

const triggerWords = {
  manual: 'Manual',
  cron: 'Schedule',
  webhook: 'Webhook',
  mcp: 'Tool check',
  agent_message: 'Agent message',
}

export function triggerLabel(kind) {
  if (!kind) return ''
  return triggerWords[kind] || sentence(kind)
}

// How long a run took, or has taken so far while it holds the agent.
export function runDuration(run, now = Date.now()) {
  const start = Date.parse(run?.created_at)
  const end = runActive(run?.status) ? now : Date.parse(run?.updated_at)
  if (!Number.isFinite(start) || !Number.isFinite(end)) return ''
  return executionTime((end - start) / 1000) ?? ''
}

// A run's one-line summary: the routine result, the error or nothing.
export function runNote(run) {
  if (run?.silent) return 'No reply needed'
  return run?.error || run?.result_summary || ''
}

export function whenLabel(value, now = Date.now()) {
  const at = Date.parse(value)
  if (!Number.isFinite(at)) return ''
  const minutes = Math.round((now - at) / 60_000)
  if (minutes < 1) return 'Just now'
  if (minutes < 60) return `${minutes}m ago`
  if (minutes < 24 * 60) return `${Math.round(minutes / 60)}h ago`
  return new Date(at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

// When a run happened or runs next, in local time with no time zone: `Today
// at 8:56 AM`, `Yesterday at 9:03 AM`, `Tomorrow at 2:00 AM` or `Oct 12 at
// 9:08 AM`, with the year when it differs.
export function runTimeLabel(value, now = Date.now()) {
  const at = new Date(value)
  if (!Number.isFinite(at.getTime())) return ''
  const today = new Date(now)
  const midnight = (date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  const days = Math.round((midnight(today) - midnight(at)) / 86_400_000)
  const day = days === 0 ? 'Today' : days === 1 ? 'Yesterday' : days === -1 ? 'Tomorrow'
    : at.toLocaleDateString(undefined, { month: 'short', day: 'numeric', ...(at.getFullYear() === today.getFullYear() ? {} : { year: 'numeric' }) })
  return `${day} at ${at.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })}`
}

// A run's title in the history: its routine's name, else what started it.
export function runTitle(run, routines = []) {
  const routine = routines.find((entry) => entry.id && entry.id === run?.trigger?.routine_id)
  if (routine) return routine.name
  if (run?.trigger?.source === 'test') return 'Test run'
  if (run?.trigger?.source === 'replay') return 'Retry'
  if (run?.trigger?.kind === 'manual') return 'Run now'
  return triggerLabel(run?.trigger?.kind) || 'Run'
}

// A run's stoplight: green for a finished reply, red for a failure and
// yellow for the rest, such as a silent reply, a wait or a run in flight.
export function runLight(run) {
  if (run?.status === 'failed') return 'red'
  return run?.status === 'completed' && !run.silent ? 'green' : 'yellow'
}

// The state a history row names after its time. A finished reply names none.
export function runOutcome(run) {
  if (run?.status === 'completed') return run.silent ? 'No reply needed' : ''
  return runStatusLabel(run?.status)
}

// What an imported template brought, as the profile's template note says.
export function templateNote(template) {
  if (!template) return ''
  const skills = (template.skills ?? []).length
  const plugins = (template.plugins ?? []).length
  return [
    template.coverage === 'public-profile' ? 'Imported from a public share page, which holds the profile only.' : 'Imported from a template.',
    skills ? `${skills} ${skills === 1 ? 'skill stays' : 'skills stay'} as reference text.` : '',
    plugins ? `${plugins} ${plugins === 1 ? 'tool needs' : 'tools need'} setup in Tools.` : '',
  ].filter(Boolean).join(' ')
}

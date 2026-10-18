// The words the agent profile, the routines and Activity show for a run and
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

// The tone a status reads in: oxide for a failure, ochre for a run that waits.
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

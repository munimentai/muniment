// The words a routine row shows for its trigger and its last result.
import { cronLabel } from './cron.js'
import { runStatusLabel, whenLabel } from '../lib/agent-labels.js'

const SENDERS = { github: 'GitHub', forgejo: 'Forgejo' }

export function triggerSummary(trigger) {
  if (trigger?.kind === 'cron') return `${cronLabel(trigger.expr)} · ${trigger.tz}`
  if (trigger?.kind === 'webhook') return `Webhook · ${SENDERS[trigger.preset] ?? 'any sender'}`
  if (trigger?.kind === 'mcp') return `${trigger.server} ${trigger.tool} · ${cronLabel(trigger.every?.expr)}`
  return trigger?.kind ?? ''
}

export function lastFiredLabel(routine, now = Date.now()) {
  if (!routine?.last_fired_at) return 'Not run yet'
  const result = routine.last_result
  const outcome = result?.silent ? 'No reply needed' : runStatusLabel(result?.status)
  return [whenLabel(routine.last_fired_at, now), outcome].filter(Boolean).join(' · ')
}

// A routine the agent's instructions proposed: still off and never fired.
export function isProposal(routine) {
  return routine?.created_from !== 'user' && !routine?.enabled && !routine?.last_fired_at
}

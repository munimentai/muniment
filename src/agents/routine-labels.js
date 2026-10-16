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

// A routine waiting for the user to accept it: core marks it `proposed`,
// from the agent's instructions or an imported template.
export function isProposal(routine) {
  return routine?.proposed === true
}

// When a tool check last asked its tool and what came of it. Core fills
// `last_poll`; a failed check names its error and the next try.
export function pollLabel(routine, now = Date.now()) {
  if (routine?.trigger?.kind !== 'mcp') return ''
  const poll = routine.last_poll
  if (!poll?.at) return 'Not checked yet'
  const when = whenLabel(poll.at, now)
  if (poll.error) {
    const retry = Date.parse(poll.retry_at ?? '')
    const next = Number.isFinite(retry) ? ` · next try ${new Date(retry).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })}` : ''
    const failures = poll.failures > 1 ? ` · ${poll.failures} failures in a row` : ''
    return `Checked ${when.toLowerCase()} · failed: ${poll.error}${failures}${next}`
  }
  return `Checked ${when.toLowerCase()} · ${poll.fired ? 'condition held, run started' : 'condition did not hold'}`
}

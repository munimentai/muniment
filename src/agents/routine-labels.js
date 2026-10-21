// The words a routine row shows for its trigger and its last result.
import { cronLabel, localTimeZone } from './cron.js'
import { runStatusLabel, runTimeLabel, whenLabel } from '../lib/agent-labels.js'

const SENDERS = { github: 'GitHub', forgejo: 'Forgejo' }

export function triggerSummary(trigger) {
  if (trigger?.kind === 'cron') return `${cronLabel(trigger.expr)} · ${trigger.tz}`
  if (trigger?.kind === 'webhook') return `Webhook · ${SENDERS[trigger.preset] ?? 'any sender'}`
  if (trigger?.kind === 'mcp') return `${trigger.server} ${trigger.tool} · ${cronLabel(trigger.every?.expr)}`
  return trigger?.kind ?? ''
}

// The icon a routine row shows for its trigger.
export function routineIcon(trigger) {
  return trigger?.kind === 'webhook' ? 'link' : trigger?.kind === 'mcp' ? 'plug-zap' : 'clock'
}

const lower = (text) => text.replace(/^[A-Z]/, (letter) => letter.toLowerCase())

// When a routine runs, with no time zone. A schedule in this computer's zone
// reads as its phrase. One in another zone names the next run that core
// reports, in local time.
export function routineWhen(routine, now = Date.now()) {
  const trigger = routine?.trigger
  if (trigger?.kind === 'cron') {
    if (trigger.tz && trigger.tz !== localTimeZone() && Number.isFinite(routine.next_fire_at)) return `Next run ${lower(runTimeLabel(routine.next_fire_at * 1000, now))}`
    return cronLabel(trigger.expr)
  }
  if (trigger?.kind === 'webhook') return `Webhook · ${SENDERS[trigger.preset] ?? 'any sender'}`
  if (trigger?.kind === 'mcp') return `${trigger.server} ${trigger.tool} · ${lower(cronLabel(trigger.every?.expr))}`
  return trigger?.kind ?? ''
}

// When the agent starts itself next, in local time with no time zone: the
// soonest time core reports for an enabled routine, else the first enabled
// routine's trigger as its routine row words it.
export function nextRunLabel(agent, now = Date.now()) {
  if (agent?.paused_reason) return 'Paused'
  const routines = (agent?.routines ?? []).filter((routine) => routine.enabled && !isProposal(routine))
  const times = routines.map((routine) => routine.next_fire_at).filter(Number.isFinite)
  if (times.length) return runTimeLabel(Math.min(...times) * 1000, now)
  return routines.length ? routineWhen(routines[0], now) : 'Manual'
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

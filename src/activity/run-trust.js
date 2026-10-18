import { actionDuration, actionGroups } from './action-feedback.js'
import { modelLabel } from '../lib/model-label.js'
import { receiptTime } from '../lib/execution-time.js'

const recorded = (value) => value !== undefined && value !== null && value !== ''
const noun = (count, one, many = `${one}s`) => `${count.toLocaleString('en-US')} ${count === 1 ? one : many}`

// 18,240 reads 18.2k; a million and more read with an M.
export function compactTokens(count) {
  const value = Number(count)
  if (!Number.isFinite(value) || value < 0) return null
  if (value < 1000) return String(Math.round(value))
  if (value < 1_000_000) return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0).replace(/\.0$/, '')}k`
  return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`
}

// The tokens a turn used: the total Pi reports, else input and output.
export function turnTokens(tokens) {
  if (!tokens) return null
  if (Number.isFinite(tokens.total) && tokens.total > 0) return tokens.total
  const sum = [tokens.input, tokens.output, tokens.cacheRead, tokens.cacheWrite].filter(Number.isFinite).reduce((total, value) => total + value, 0)
  return sum > 0 ? sum : null
}

// A catalog estimate in dollars, never a bill: cents for a cent or more, else
// a tenth of a cent, else under a tenth of a cent.
export function estimateCost(usd) {
  const value = Number(usd)
  if (!recorded(usd) || !Number.isFinite(value) || value < 0) return null
  if (value === 0) return '$0.00'
  if (value >= 0.01) return `$${value.toFixed(2)}`
  if (value >= 0.001) return `$${value.toFixed(3)}`
  return '<$0.001'
}

// One turn's run line: machine, model, permission mode, elapsed time, tokens
// and the estimated cost, each only when the run recorded it.
export function runLineParts(run, { machine = null, modes = [], elapsed = null, liveMode = null } = {}) {
  const receipt = run?.receipt ?? null
  const mode = modes.find((candidate) => candidate.id === receipt?.permission_mode) ?? null
  const tokens = turnTokens(receipt?.tokens)
  const time = recorded(receipt?.time) ? receiptTime(receipt.time) : elapsed
  return {
    machine,
    model: recorded(receipt?.model) ? modelLabel(receipt.model) : null,
    mode: mode?.title ?? liveMode,
    time,
    tokens: tokens === null ? null : `${compactTokens(tokens)} tokens`,
    cost: estimateCost(receipt?.cost_usd),
  }
}

// What a screen reader hears for the line, in words.
export function runLineLabel(parts) {
  return [
    parts.machine && `On ${parts.machine}`,
    parts.model && `model ${parts.model}`,
    parts.mode && `mode ${parts.mode}`,
    parts.time && `took ${parts.time}`,
    parts.tokens,
    parts.cost && `${parts.cost} API estimate`,
  ].filter(Boolean).join(', ')
}

// Every tool call of a turn in order, with its state and duration.
export function runTimeline(toolActivity = [], live = false) {
  return actionGroups(toolActivity, live).flatMap((group) => group.actions.map((action) => ({
    id: action.effectId,
    icon: action.icon,
    label: action.label,
    state: action.state,
    duration: action.startedAt && action.finishedAt ? actionDuration(action.startedAt, action.finishedAt) : '',
  })))
}

const EFFECT_WORDS = {
  command: ['command ran', 'commands ran', 'Their changes inside the folder are restored. Anything else they did stays.'],
  network: ['web request went out', 'web requests went out', 'What they sent stays sent.'],
  message: ['message went to another agent', 'messages went to other agents', 'Sent messages stay sent.'],
  external: ['tool of another service ran', 'tools of other services ran', 'Their effects stay.'],
  outside_folder: ['file outside the folder changed', 'files outside the folder changed', 'They stay as they are.'],
}

// The effects a rewind leaves in place, one line per kind.
export function irreversibleLines(groups = []) {
  return groups.flatMap((group) => {
    const words = EFFECT_WORDS[group.kind]
    if (!words || !group.count) return []
    return [{ kind: group.kind, headline: `${group.count} ${group.count === 1 ? words[0] : words[1]}.`, note: words[2], labels: group.labels ?? [] }]
  })
}

const UNRESTORABLE = {
  too_large: 'too large to save',
  link: 'a link, which checkpoints do not follow',
  unreadable: 'could not be read',
  blocked: 'a folder now stands there',
}

export function unrestorableReason(reason) {
  return UNRESTORABLE[reason] ?? 'cannot be restored'
}

// The file part of a rewind preview: what returns, what goes and what stays.
export function rewindFiles(files) {
  if (!files) return { available: false, note: 'Files cannot be restored for this turn.', restore: [], remove: [], cannot: [], notCovered: [] }
  if (!files.files_available) return { available: false, note: files.unavailable ?? 'Files cannot be restored for this turn.', restore: [], remove: [], cannot: [], notCovered: files.not_covered ?? [] }
  return {
    available: true,
    note: files.restore.length || files.remove.length ? '' : 'No file changed since this message.',
    restore: files.restore,
    remove: files.remove,
    cannot: (files.cannot_restore ?? []).map((item) => ({ path: item.path, reason: unrestorableReason(item.reason) })),
    notCovered: files.not_covered ?? [],
  }
}

// The one line a finished rewind reports, and its details. A rewind is done
// only when core says every part of it was.
export function rewindOutcome(result) {
  const files = result?.files ?? null
  const failed = files?.failed ?? []
  const cannot = files?.cannot_restore ?? []
  const conversationFailed = result?.conversation && typeof result.conversation === 'object'
  const parts = []
  if (files) {
    if (files.restored?.length) parts.push(`restored ${noun(files.restored.length, 'file')}`)
    if (files.removed?.length) parts.push(`removed ${noun(files.removed.length, 'file')}`)
  }
  const done = result?.status === 'rewound'
  const turns = Number(result?.turns) || 0
  const lead = done ? `Rewound ${noun(turns, 'turn')}` : `Partly rewound ${noun(turns, 'turn')}`
  const details = [
    ...failed.map((item) => `${item.path}: ${item.reason}`),
    ...cannot.map((item) => `${item.path}: ${unrestorableReason(item.reason)}`),
  ]
  if (conversationFailed) details.unshift(`The saved conversation was not restored: ${result.conversation.failed}`)
  if (!files) details.push('Files were left as they are.')
  const headline = `${lead}${parts.length ? `, ${parts.join(' and ')}` : ''}.`
  return { done, headline, details, notCovered: files?.not_covered ?? [] }
}

// Why a rewind of this message is not offered now, or null when it is.
export function rewindUnavailable({ owner = null, replica = false, group = false, running = false } = {}) {
  if (group) return 'Group threads cannot rewind.'
  if (replica) return `${owner?.name ?? 'Another machine'} runs this thread. Rewind it there.`
  if (running) return 'Stop the reply to rewind.'
  return null
}

// The state of one changed file in words.
export function turnFileState(state) {
  return state === 'reverted' ? 'Reverted' : state === 'changed_since' ? 'Changed since' : ''
}

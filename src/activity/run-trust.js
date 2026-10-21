const noun = (count, one, many = `${one}s`) => `${count.toLocaleString('en-US')} ${count === 1 ? one : many}`

// The receipt rows for where a turn ran: its machine, its permission mode
// and where the model and the mode come from. `sources` names those layers
// for the newest turn while the thread still runs on them, such as
// `from project Homelab`.
export function runContextRows(run, { machine = null, modes = [], sources = {} } = {}) {
  const mode = modes.find((candidate) => candidate.id === run?.receipt?.permission_mode)?.title ?? null
  return [
    machine && { label: 'Machine', value: machine },
    mode && { label: 'Mode', value: sources.mode ? `${mode} (${sources.mode})` : mode },
    sources.model && { label: 'Model source', value: sources.model },
  ].filter(Boolean).map((row) => ({ ...row, route: false }))
}

// Each turn's sources as its footer named them while it was the newest
// turn, so a later turn leaves an earlier footer as it was. `current` reads
// the newest turn's sources.
export function turnSources() {
  const seen = new Map()
  return (run, newestRunId, current) => {
    if (!run?.id) return {}
    if (run.id !== newestRunId) return seen.get(run.id) ?? {}
    const sources = current(run)
    seen.set(run.id, sources)
    return sources
  }
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

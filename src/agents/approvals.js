// The controls an agent approval offers and the line its answer earns.
// Core puts the exact operation in the approval (`summary`, `rule`) and the
// answers it accepts (`answers`), routes each answer to the machine that runs
// the agent and keeps the first one. An approval from an older runtime has
// neither, so its gate's own options answer it.

export const ANSWER_LABELS = {
  allow_once: 'Allow once',
  allow_thread: 'Allow for this thread',
  always_allow_agent: 'Always allow for this agent',
  always_allow: 'Always allow',
  deny: 'Deny',
  // A plan approval's answers: approve it into a mode, or keep planning.
  plan_manual: 'Approve, then ask before changes',
  plan_edit_auto: 'Approve and edit automatically',
  plan_auto: 'Approve with auto review',
  plan_keep: 'Keep planning',
}
const ORDER = Object.keys(ANSWER_LABELS)

const ALLOW_ONCE = 'Allow once'
const ALLOW_THREAD = 'Allow for this thread'
const DENY = 'Deny'

// `[{ label, answer, tone }]` for one approval. Only the answers core lists
// appear, in a fixed order, so an agent whose ask-first rule matches offers
// no "Always allow for this agent". A request that asks for text or a file
// review opens its conversation instead.
export function approvalChoices(approval) {
  if (Array.isArray(approval?.answers) && approval.answers.length) {
    return ORDER.filter((id) => approval.answers.includes(id))
      .map((id) => ({ label: ANSWER_LABELS[id], answer: id, ...(id === 'deny' || id === 'plan_keep' ? { tone: 'deny' } : {}) }))
  }
  const request = approval?.request
  if (request?.kind === 'confirm') {
    return [
      { label: ALLOW_ONCE, answer: { type: 'confirm', value: true } },
      { label: DENY, answer: { type: 'confirm', value: false }, tone: 'deny' },
    ]
  }
  if (request?.kind === 'select') {
    const options = request.options ?? []
    if (options.includes(ALLOW_ONCE) && options.includes(DENY)) {
      return [
        { label: ALLOW_ONCE, answer: { type: 'select', value: ALLOW_ONCE } },
        ...(options.includes(ALLOW_THREAD) ? [{ label: ALLOW_THREAD, answer: { type: 'select', value: ALLOW_THREAD } }] : []),
        { label: DENY, answer: { type: 'select', value: DENY }, tone: 'deny' },
      ]
    }
    return options.map((option) => ({ label: option, answer: { type: 'select', value: option } }))
  }
  return []
}

const json = (value) => JSON.stringify(value ?? {}, null, 2)

// The inputs of the exact operation, as `[label, value]` rows.
export function ruleInputs(rule) {
  const resource = rule?.resource
  if (!resource) return []
  if (resource.type === 'command') {
    const [shell, ...rest] = resource.arguments ?? []
    return [['Command', rest.join(' ')], ['Shell', shell ?? ''], ['Folder', resource.working_directory ?? '']].filter(([, value]) => value)
  }
  if (resource.type === 'path') return [['Action', resource.operation ?? ''], ['Path', resource.path ?? '']].filter(([, value]) => value)
  if (resource.type === 'tool') {
    const input = resource.input && Object.keys(resource.input).length ? json(resource.input) : ''
    return [['Tool', resource.name ?? ''], ['Input', input]].filter(([, value]) => value)
  }
  return [['Request', json(rule)]]
}

// The operation an approval asks about and its inputs. `operation` is core's
// one-line summary, `rows` the exact rule's inputs. An older approval names
// the gate's title and its message.
export function approvalSummary(approval) {
  const request = approval?.request ?? {}
  const rule = approval?.rule ?? (request.policy ? { request_kind: request.policy.request_kind, resource: request.policy.resource } : null)
  const operation = String(approval?.summary ?? request.policy?.summary ?? '').trim()
  const title = String(request.title ?? 'Approval needed').trim()
  const rows = ruleInputs(rule)
  if (rows.length) return { operation: operation || title, title, rows, exact: true }
  const inputs = request.kind === 'confirm' ? request.message ?? '' : request.kind === 'select' ? '' : request.prefill ?? request.placeholder ?? ''
  const text = String(inputs ?? '').trim()
  return { operation: operation || title, title, rows: text ? [['Input', text]] : [], exact: false }
}

// A value long enough to fold behind Show all.
export function longInput(value, lines = 6, chars = 280) {
  const text = String(value ?? '')
  return text.length > chars || text.split('\n').length > lines
}

export function approvalOutcome(status) {
  if (status === 'already_answered') return 'Another device answered this first. Its answer stands.'
  if (status === 'sent') return 'Answer sent. The machine applies it when it answers.'
  return ''
}

// The pending approvals that belong in one thread's transcript.
export function threadApprovals(approvals = [], threadId) {
  return threadId ? approvals.filter((approval) => approval.thread_id === threadId) : []
}

// The title of the gate a submitted plan opens.
export const PLAN_GATE_TITLE = 'Approve this plan?'

export function isPlanGate(gate) {
  return gate?.kind === 'select' && gate.title === PLAN_GATE_TITLE
}

// The answers a composer thread's select gate shows beside its Deny control.
// A thread without an agent never offers "Always allow for this agent", and
// a gate whose options name Deny answers Deny with that option.
export function composerGateOptions(gate, hasAgent = false) {
  const options = (gate?.options ?? []).filter((option) => hasAgent || option !== ANSWER_LABELS.always_allow_agent)
  const deny = options.includes(DENY) ? { type: 'select', value: DENY } : { type: 'cancelled' }
  return { options: options.filter((option) => option !== DENY), deny }
}

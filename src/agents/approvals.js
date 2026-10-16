// The controls an agent approval offers and the line its answer earns.
// Core routes each answer to the machine that runs the agent and keeps the
// first one. "Always allow" answers the gate's "Allow for this thread", which
// journals the exact allow policy in the agent's thread through core.

const ALLOW_ONCE = 'Allow once'
const ALLOW_THREAD = 'Allow for this thread'
const DENY = 'Deny'

// `[{ label, answer, tone }]` for one approval's request. A request that asks
// for text or a file review opens its conversation instead.
export function approvalChoices(request) {
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
        ...(options.includes(ALLOW_THREAD) ? [{ label: 'Always allow', answer: { type: 'select', value: ALLOW_THREAD } }] : []),
        { label: DENY, answer: { type: 'select', value: DENY }, tone: 'deny' },
      ]
    }
    return options.map((option) => ({ label: option, answer: { type: 'select', value: option } }))
  }
  return []
}

// The operation an approval asks about, and its inputs, as the gate names them.
export function approvalSummary(approval) {
  const request = approval?.request ?? {}
  const title = String(request.title ?? 'Approval needed').trim()
  const inputs = request.kind === 'confirm' ? request.message ?? '' : request.kind === 'select' ? '' : request.prefill ?? request.placeholder ?? ''
  return { title, inputs: String(inputs ?? '').trim() }
}

export function approvalOutcome(status) {
  if (status === 'already_answered') return 'Another device answered this first. Its answer stands.'
  if (status === 'sent') return 'Answer sent. The machine applies it when it answers.'
  return ''
}

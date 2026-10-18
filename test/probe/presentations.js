// Core's presentations of the tool calls in
// `core/protocol-fixtures/tool-presentation/calls.json`, which a core test
// keeps current. The probe fixtures carry them as the runtime would, keyed by
// effect id and approval id, and the tools gallery renders them all.
import presented from '../../core/protocol-fixtures/tool-presentation/presented.json' with { type: 'json' }

const calls = new Map(presented.calls.map((call) => [call.effectId, call]))
const gates = new Map(presented.gates.map((gate) => [gate.id, gate]))

export const galleryCalls = () => structuredClone(presented.calls)
export const galleryGates = () => structuredClone(presented.gates)

// Gives each call core described its presentation, in place.
export function presentActivities(activities = []) {
  for (const activity of activities) {
    const known = calls.get(activity.effectId)
    if (known && !activity.presentation) activity.presentation = structuredClone(known.presentation)
  }
  return activities
}

// An approval with core's summary and presentation of its rule.
export function presentApproval(approval) {
  const gate = gates.get(approval.approval_id)
  return gate ? { ...approval, summary: gate.summary, presentation: structuredClone(gate.presentation) } : approval
}

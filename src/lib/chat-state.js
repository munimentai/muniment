export function composerAction(event, text, active) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing || !text.trim()) return null
  if (!active) return 'submit'
  return active.id === 'pending' ? null : 'steer'
}

export function receiptParts(receipt = {}) {
  const parts = [receipt.route, receipt.model, receipt.cost, receipt.time].filter(Boolean)
  for (const capability of receipt.capabilities ?? []) {
    if (capability?.name && capability?.version) parts.push(`${capability.name}@${capability.version}`)
  }
  return parts
}

export function receiptRows(receipt = {}) {
  const rows = []
  for (const [field, label] of [['route', 'Route'], ['model', 'Model'], ['cost', 'Cost'], ['time', 'Time']]) {
    if (receipt[field] !== undefined && receipt[field] !== null) rows.push({ label, value: receipt[field], route: field === 'route' })
  }
  for (const capability of receipt.capabilities ?? []) {
    if (capability?.name !== undefined && capability?.name !== null && capability?.version !== undefined && capability?.version !== null) {
      rows.push({ label: 'Capability', value: `${capability.name}@${capability.version}`, route: false })
    }
  }
  return rows
}

export function applyChatEvent(run, event) {
  if (!run || event.runId !== run.id) return run
  if (event.phase) return { ...run, phase: event.phase, text: event.text ?? '', receipt: event.receipt ?? null }
  if (event.type === 'prompt-accepted') return { ...run, accepted: true }
  if (event.type === 'text-delta') return { ...run, phase: 'streaming', text: run.text + event.text }
  if (event.type === 'completed') return { ...run, phase: 'complete', receipt: event.receipt ?? {} }
  if (event.type === 'cancelled') return { ...run, phase: 'cancelled' }
  if (event.type === 'failed') return { ...run, phase: 'failed' }
  return run
}

export function applyBufferedChatEvents(run, events) {
  return events.reduce((projection, event) => applyChatEvent(projection, event), run)
}

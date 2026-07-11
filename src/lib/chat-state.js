export function shouldSend(event, text, active) {
  return event.key === 'Enter' && !event.shiftKey && !event.isComposing && text.trim().length > 0 && !active
}

export function receiptParts(receipt = {}) {
  const parts = [receipt.route, receipt.model, receipt.cost, receipt.time].filter(Boolean)
  for (const capability of receipt.capabilities ?? []) {
    if (capability?.name && capability?.version) parts.push(`${capability.name}@${capability.version}`)
  }
  return parts
}

export function applyChatEvent(run, event) {
  if (!run || event.runId !== run.id) return run
  if (event.type === 'prompt-accepted') return { ...run, accepted: true }
  if (event.type === 'text-delta') return { ...run, phase: 'streaming', text: run.text + event.text }
  if (event.type === 'completed') return { ...run, phase: 'complete', receipt: event.receipt ?? {} }
  if (event.type === 'cancelled') return { ...run, phase: 'cancelled' }
  if (event.type === 'failed') return { ...run, phase: 'failed' }
  return run
}

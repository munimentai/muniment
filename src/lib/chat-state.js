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

export function toolName(activity = {}) {
  return activity.displayName?.trim() || 'Tool activity'
}

export function toolStatus(activity = {}) {
  return ['running', 'completed', 'failed'].includes(activity.status) ? activity.status : 'status unknown'
}

const generating = 'Generating a reply.'

const runPhaseAnnouncements = {
  thinking: generating,
  streaming: generating,
  // A run paused on a permission decision has no affordance to announce yet, so it
  // stays inside the same coarse in-progress state rather than inventing copy.
  'pending-permission': generating,
  resuming: 'Resuming the interrupted reply.',
  cancelled: 'Reply stopped.',
  failed: 'Reply failed.',
  interrupted: 'Reply interrupted.',
}

// What a screen reader hears about a run. Streamed text never reaches the live
// region: every in-progress phase maps to the same coarse string, so the
// announcement changes once when a run starts and once when it settles.
export function runAnnouncement(run) {
  if (!run) return ''
  if (run.phase === 'complete') return `Reply complete. ${run.text ?? ''}`.trim()
  return runPhaseAnnouncements[run.phase] ?? ''
}

export function formatByteSize(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return 'Unknown size'
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`
}

export function applyChatEvent(run, event) {
  if (!run || event.runId !== run.id) return run
  if (event.phase) return { ...run, phase: event.phase, text: event.text ?? '', receipt: event.receipt ?? null, toolActivity: event.toolActivity ?? [], attachments: event.attachments ?? run.attachments ?? [] }
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

export function historyMessages(history) {
  return history.flatMap((entry) => [
    ...(entry.prompt || entry.attachments?.length ? [{ role: 'user', text: entry.prompt ?? '', attachments: entry.attachments ?? [] }] : []),
    { role: 'assistant', run: { id: entry.runId, phase: entry.phase, text: entry.text, receipt: entry.receipt ?? null, prompt: entry.prompt ?? '', toolActivity: entry.toolActivity ?? [], resumable: entry.resumable === true } },
  ])
}

export function composerAction(event, text, active) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing || !text.trim()) return null
  if (!active) return 'submit'
  return active.id === 'pending' ? null : 'steer'
}

export function permissionGateAction(event, kind, platform = navigator.platform) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return null
  if (kind === 'input') return event.metaKey || event.ctrlKey || event.altKey ? null : 'commit'
  if (kind !== 'editor' || event.altKey) return null
  const mac = platform.startsWith('Mac')
  return (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) ? 'commit' : null
}

// A receipt field is recorded when the server sent something to show. Zero is a
// record (a route can genuinely cost nothing); a blank is a gap and would only
// render as a stray separator.
const recorded = (value) => value !== undefined && value !== null && value !== ''

function receiptTrailing(receipt) {
  const trailing = [receipt?.cost, receipt?.time].filter(recorded)
  for (const capability of receipt?.capabilities ?? []) {
    if (recorded(capability?.name) && recorded(capability?.version)) trailing.push(`${capability.name}@${capability.version}`)
  }
  return trailing
}

// The provenance summary — `route → model · cost · time` (design-spec §2.2).
// Route stays a named field rather than the head of a flat array: §1.2 permits
// --signal on the route segment only, so a receipt without a route must never
// paint whatever follows green.
export function receiptSummary(receipt = {}) {
  const route = recorded(receipt?.route) ? receipt.route : null
  const model = recorded(receipt?.model) ? receipt.model : null
  const detail = [model, ...receiptTrailing(receipt)].filter(recorded)
  // The arrow states the route→model relation and nothing else; any other
  // neighbour of the route takes the plain separator.
  const separator = route === null || detail.length === 0 ? '' : model === null ? ' · ' : ' → '
  return { route, separator, detail: detail.join(' · ') }
}

// What a screen reader hears instead of the summary: engines pronounce →
// inconsistently, so the accessible name states the relation in words.
export function receiptLabel(receipt = {}) {
  const route = recorded(receipt?.route) ? receipt.route : null
  const model = recorded(receipt?.model) ? receipt.model : null
  const relation = route !== null && model !== null ? `Routed via ${route} to model ${model}`
    : route !== null ? `Routed via ${route}`
    : model !== null ? `Model ${model}`
    : null
  return [relation, ...receiptTrailing(receipt)].filter(recorded).join(', ')
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
  'pending-permission': 'Waiting for your decision.',
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
  const pendingPermission = event.pendingPermission ?? null
  if (event.phase) return { ...run, phase: event.phase, text: event.text ?? '', receipt: event.receipt ?? null, toolActivity: event.toolActivity ?? [], attachments: event.attachments ?? run.attachments ?? [], pendingPermission }
  if (event.type === 'prompt-accepted') return { ...run, accepted: true, pendingPermission }
  if (event.type === 'text-delta') return { ...run, phase: 'streaming', text: run.text + event.text, pendingPermission }
  if (event.type === 'completed') return { ...run, phase: 'complete', receipt: event.receipt ?? {}, pendingPermission }
  if (event.type === 'cancelled') return { ...run, phase: 'cancelled', pendingPermission }
  if (event.type === 'failed') return { ...run, phase: 'failed', pendingPermission }
  return run
}

export function applyBufferedChatEvents(run, events) {
  return events.reduce((projection, event) => applyChatEvent(projection, event), run)
}

export function historyMessages(history) {
  return history.flatMap((entry) => [
    ...(entry.prompt || entry.attachments?.length ? [{ role: 'user', text: entry.prompt ?? '', attachments: entry.attachments ?? [] }] : []),
    { role: 'assistant', run: { id: entry.runId, phase: entry.phase, text: entry.text, receipt: entry.receipt ?? null, prompt: entry.prompt ?? '', toolActivity: entry.toolActivity ?? [], pendingPermission: entry.pendingPermission ?? null, resumable: entry.resumable === true } },
  ])
}

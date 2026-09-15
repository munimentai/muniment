import { shortcutDisplayLabel } from './artifact-rail-state.js'

export function composerAction(event, text, active) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing || !text.trim()) return null
  if (!active) return 'submit'
  return active.id === 'pending' ? null : 'steer'
}

export function permissionGateCommitHint(kind, platform = navigator.platform) {
  if (kind !== 'editor') return null
  const shortcut = platform.startsWith('Mac') ? 'Meta+⏎' : 'Control+⏎'
  return `${shortcutDisplayLabel(shortcut)} submits`
}

export function permissionGateAction(event, kind, platform = navigator.platform) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return null
  if (kind === 'input') return event.metaKey || event.ctrlKey || event.altKey ? null : 'commit'
  if (kind !== 'editor' || event.altKey) return null
  const mac = platform.startsWith('Mac')
  return (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) ? 'commit' : null
}

export function codeDiffPermissionAnswer(gate) {
  return {
    type: 'codeDiff',
    value: {
      gate_id: gate.gateId,
      effect_id: gate.effect_id,
      code_diff_id: gate.code_diff_id,
      diff_sha256: gate.diff_sha256,
      write_plan_sha256: gate.write_plan_sha256,
    },
  }
}

// A receipt field is recorded when the server sent something to show. Zero is a
// record (a route can genuinely cost nothing); a blank is a gap and would only
// render as a stray separator.
const recorded = (value) => value !== undefined && value !== null && value !== ''

const count = (value) => Number(value).toLocaleString('en-US')

// A model id reads as words on the line: hyphens become spaces, and the
// provider keeps its slash. The receipt keeps the raw id.
export function modelLabel(model) {
  return recorded(model) ? String(model).replaceAll('-', ' ') : null
}

// The provenance line is `route → model` and the clock time, nothing else. The
// rows carry the rest, and nothing appears twice. Route stays a named field
// rather than the head of a flat array: §1.2 permits --signal on the route
// segment only, so a receipt without a route must never paint whatever
// follows green.
export function receiptSummary(receipt = {}) {
  const route = recorded(receipt?.route) ? receipt.route : null
  const model = modelLabel(receipt?.model)
  const time = recorded(receipt?.time) ? receipt.time : null
  return { route, model, time }
}

// What a screen reader hears instead of the line: engines pronounce →
// inconsistently, so the accessible name states the relation in words.
export function receiptLabel(receipt = {}) {
  const { route, model, time } = receiptSummary(receipt)
  const relation = route !== null && model !== null ? `Routed via ${route} to model ${model}`
    : route !== null ? `Routed via ${route}`
    : model !== null ? `Model ${model}`
    : null
  return [relation, time].filter(recorded).join(', ')
}

// The rows under the line: everything the line does not show, in record order.
export function receiptRows(receipt = {}, recalls = []) {
  const rows = []
  if (recorded(receipt?.cost)) rows.push({ label: 'Cost', value: receipt.cost, route: false })
  const tokens = receipt?.tokens
  if (tokens && recorded(tokens.input) && recorded(tokens.output)) {
    const parts = [`${count(tokens.input)} in`, `${count(tokens.output)} out`]
    if (Number(tokens.cacheRead) > 0) parts.push(`${count(tokens.cacheRead)} cached`)
    if (Number(tokens.cacheWrite) > 0) parts.push(`${count(tokens.cacheWrite)} written to cache`)
    if (Number(tokens.reasoning) > 0) parts.push(`${count(tokens.reasoning)} reasoning`)
    rows.push({ label: 'Tokens', value: parts.join(', '), route: false })
  }
  if (recorded(receipt?.turns)) rows.push({ label: 'Turns', value: count(receipt.turns), route: false })
  const tools = (receipt?.tools ?? []).filter((tool) => recorded(tool?.name) && recorded(tool?.calls))
  if (tools.length) {
    const parts = tools.map((tool) => `${tool.name} ${count(tool.calls)}${Number(tool.failed) > 0 ? ` (${count(tool.failed)} failed)` : ''}`)
    rows.push({ label: 'Tools', value: parts.join(', '), route: false })
  }
  for (const recall of recalls ?? []) {
    const files = recall?.files ?? []
    const total = files.length
    rows.push({ label: 'Memory', value: `${recall?.query ?? ''}, ${total} ${total === 1 ? 'file' : 'files'}`, files, route: false })
  }
  for (const capability of receipt?.capabilities ?? []) {
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

// The in-flight word is the plain verb of the run's last event. It names no
// vendor, no model and no harness, and it never rotates.
const toolVerbs = new Map([
  ['read', 'Reading'], ['grep', 'Reading'], ['find', 'Reading'], ['ls', 'Reading'],
  ['write', 'Editing'], ['edit', 'Editing'],
  ['bash', 'Running'], ['powershell', 'Running'],
  ['web_search', 'Searching'], ['fetch_content', 'Searching'],
  ['subagent', 'Delegating'],
  ['mcp', 'Calling a server'],
])

export function toolVerb(name = '') {
  const tool = String(name ?? '').trim()
  if (toolVerbs.has(tool)) return toolVerbs.get(tool)
  if (tool.startsWith('bg_')) return 'Working in the background'
  const [prefix, server] = tool.split('__')
  if (prefix === 'mcp' && server) return `Calling ${server}`
  return 'Working'
}

const stageWords = { routing: 'Routing', thinking: 'Thinking', writing: 'Writing' }

export function stageWord(stage = 'routing') {
  if (typeof stage === 'string' && stage.startsWith('tool:')) return toolVerb(stage.slice(5))
  return stageWords[stage] ?? stageWords.routing
}

// A projection names what the run does now: the tool that runs, text that grew
// since the last projection, or the thought between them. Routing holds until
// the runtime reports the started turn.
export function runStage(previous, next) {
  const running = [...(next.toolActivity ?? [])].reverse().find((tool) => tool.status === 'running')
  if (running) return `tool:${running.displayName ?? ''}`
  // A restored run has no earlier projection: its phase names the word.
  if (!previous) return next.phase === 'streaming' ? 'writing' : next.turnStarted || next.text ? 'thinking' : 'routing'
  if ((next.text ?? '').length > (previous.text ?? '').length) return 'writing'
  if ((previous.toolActivity ?? []).some((tool) => tool.status === 'running')) return 'thinking'
  if (previous.stage && previous.stage !== 'routing') return previous.stage
  return next.turnStarted ? 'thinking' : 'routing'
}

const generating = 'Generating a reply.'

const runPhaseAnnouncements = {
  'acquiring-pi': 'Reply setup has started. Please wait.',
  thinking: generating,
  streaming: generating,
  'pending-permission': 'Waiting for your decision.',
  resuming: 'Resuming the interrupted reply.',
  cancelled: 'Reply stopped.',
  interrupted: 'Reply interrupted.',
}

// Strip known retry guidance, not punctuation inside file names or recorded causes.
const retryGuidance = new RegExp(`(?:^|(?<=[.!?])\\s)(?:${[
  'Try again',
  'Check the key and try again',
  'Check the files and try again',
  'Choose a smaller image before sending again',
  'Remove an image before sending again',
  'Remove images or choose smaller images before sending again',
  'Choose a PNG, JPEG, GIF, or WebP image before sending again',
].join('|')})\\.?$`, 'i')

// The retry button carries the next step, not the recorded cause.
export function runFailureMessage(run) {
  const reason = typeof run?.failureReason === 'string'
    ? run.failureReason.trim().replace(/\s+/g, ' ').replace(retryGuidance, '').trim()
    : ''
  if (!reason) return 'Reply failed.'
  return /[.!?]$/.test(reason) ? reason : `${reason}.`
}

// Streamed text never reaches the live region until the run settles.
export function runAnnouncement(run) {
  if (!run) return ''
  if (run.phase === 'failed') return runFailureMessage(run)
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
  if (event.phase) return { ...run, promptStorageNotice: event.promptStorageNotice ?? run.promptStorageNotice ?? null, phase: event.phase, stage: runStage(run, event), turnStarted: event.turnStarted === true, failureReason: event.failureReason ?? null, text: event.text ?? '', receipt: event.receipt ?? null, recalls: event.recalls ?? [], toolActivity: event.toolActivity ?? [], attachments: event.attachments ?? run.attachments ?? [], appliedDiffs: event.appliedDiffs ?? [], pendingPermission }
  if (event.type === 'prompt-accepted') return { ...run, accepted: true, pendingPermission }
  if (event.type === 'text-delta') return { ...run, phase: 'streaming', text: run.text + event.text, pendingPermission }
  if (event.type === 'completed') return { ...run, phase: 'complete', receipt: event.receipt ?? {}, recalls: event.recalls ?? [], appliedDiffs: event.appliedDiffs ?? [], pendingPermission }
  if (event.type === 'cancelled') return { ...run, phase: 'cancelled', pendingPermission }
  if (event.type === 'failed') return { ...run, phase: 'failed', failureReason: event.failureReason ?? null, pendingPermission }
  return run
}

export function applyBufferedChatEvents(run, events) {
  return events.reduce((projection, event) => applyChatEvent(projection, event), run)
}

export function historyMessages(history) {
  return history.flatMap((entry) => [
    ...(entry.prompt || entry.attachments?.length ? [{ role: 'user', text: entry.prompt ?? '', attachments: entry.attachments ?? [] }] : []),
    { role: 'assistant', run: { id: entry.runId, promptStorageNotice: entry.promptStorageNotice ?? null, phase: entry.phase, stage: runStage(null, entry), turnStarted: entry.turnStarted === true, failureReason: entry.failureReason ?? null, text: entry.text, receipt: entry.receipt ?? null, recalls: entry.recalls ?? [], prompt: entry.prompt ?? '', toolActivity: entry.toolActivity ?? [], appliedDiffs: entry.appliedDiffs ?? [], pendingPermission: entry.pendingPermission ?? null, resumable: entry.resumable === true } },
  ])
}

// The phases a run reaches once it stops. src-tauri/src/chat.rs projection_phase
// answers one of these for a settled run and thinking, streaming, or
// pending-permission for a run that still executes (ADR 0012, desktop run rejoin).
export const settledPhases = new Set(['complete', 'cancelled', 'failed', 'interrupted'])

// The run the runtime still drives in a projected message list. A thread whose
// runs all settled answers null, so the desktop leaves the active run null.
export function unsettledRun(messages = []) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const run = messages[index]?.run
    if (run && !settledPhases.has(run.phase)) return run
  }
  return null
}

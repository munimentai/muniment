// Core describes every tool call as a presentation: its kind and icon, a
// headline, the words while it runs, labeled details and how it ended. The
// desktop renders it as it comes. A record from a runtime that sends none
// shows the tool's name.

export function presentationOf(activity = {}) {
  if (activity?.presentation?.headline) return activity.presentation
  const name = String(activity?.displayName ?? '').trim()
  return { kind: 'other', category: 'other', icon: 'wrench', headline: name || 'Tool call', progress: 'Working', details: [] }
}

// A headline as text and code parts: core marks a command or pattern in
// backticks.
export function headlineParts(text = '') {
  return String(text).split('`').map((part, index) => ({ text: part, code: index % 2 === 1 })).filter((part) => part.text)
}

// The words one call shows now: what it does while it runs, else what it
// did, and its state when it did not finish.
export function callLine(activity, live = false) {
  const presentation = presentationOf(activity)
  const running = activity?.status === 'running'
  const active = running && live
  const state = running ? (live ? 'In progress' : 'Interrupted') : activity?.status === 'failed' ? 'Failed' : 'Completed'
  return {
    presentation,
    running: active,
    state,
    label: active ? presentation.progress || presentation.headline : presentation.headline,
    result: running ? '' : presentation.result ?? '',
    failed: activity?.status === 'failed',
  }
}

// The raw input as the details show it: JSON indented, other text as is.
export function rawInput(input) {
  if (input === undefined || input === null || input === '') return ''
  if (typeof input !== 'string') return JSON.stringify(input, null, 2)
  try {
    const value = JSON.parse(input)
    return value && typeof value === 'object' ? JSON.stringify(value, null, 2) : input
  } catch {
    return input
  }
}

// The raw input of a gate's exact request: the command, the file operation
// or the tool's input.
export function ruleInput(rule) {
  const resource = rule?.resource
  if (!resource) return ''
  if (resource.type === 'command') return rawInput({ command: (resource.arguments ?? []).slice(1).join(' '), shell: resource.arguments?.[0], folder: resource.working_directory })
  if (resource.type === 'path') return rawInput({ operation: resource.operation, path: resource.path })
  if (resource.type === 'tool') return rawInput({ tool: resource.name, input: resource.input ?? {} })
  return rawInput(rule)
}

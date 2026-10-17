// The permission modes an agent card offers. Core reports the list through
// `permission.modes` as `[{ id, title, description, available, reason? }]`,
// and the card renders whatever list it gets.
// TODO(core modes): read the list from core's `permission.modes` and drop this
// constant. Until then a mode with `legacy` saves as the agent's current
// `permissions.mode` value (`ask`, `auto` or `strict`), which core migrates.
export const PERMISSION_MODES = [
  { id: 'manual', legacy: 'ask', title: 'Manual', description: 'Reads run. Edits, commands, web and tools ask first.', available: true },
  { id: 'edit_auto', title: 'Edit automatically', description: 'File edits in the workspace run. Commands, web and tools ask.', available: true },
  { id: 'plan', title: 'Plan', description: 'Explores read-only and proposes a plan for your approval.', available: true },
  { id: 'auto', legacy: 'auto', title: 'Auto', description: 'A safety check approves routine actions and pauses anything risky.', available: true },
  { id: 'dont_ask', legacy: 'strict', title: "Don't ask", description: 'Only pre-approved actions run. Everything else is refused, for unattended agents.', available: true },
  { id: 'bypass', title: 'Bypass permissions', description: 'Everything runs except deny rules. Use only in disposable containers.', available: false, reason: 'This machine has not turned it on.' },
]

const ICONS = { manual: 'hand', edit_auto: 'pencil', plan: 'map', auto: 'zap', dont_ask: 'shield-check', bypass: 'triangle-alert' }

export function modeIcon(mode) {
  return ICONS[mode?.id] ?? 'shield-check'
}

// The mode an agent is in: its saved `permissions.mode`, read as a mode id or
// as the older value a mode replaces. An agent with none is in the first mode.
export function agentMode(agent, modes = PERMISSION_MODES) {
  const saved = agent?.permissions?.mode ?? 'ask'
  return modes.find((mode) => mode.id === saved || mode.legacy === saved) ?? null
}

// The permissions an agent saves to enter a mode. Its rules stay as they are.
export function modePermissions(agent, mode) {
  const permissions = agent?.permissions ?? { mode: 'ask', always_allow: [], ask_first: [] }
  return { ...permissions, mode: mode.legacy ?? mode.id }
}

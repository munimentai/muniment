// How the layer core names for a value reads beside it, as in
// `GPT-6 Astra · from project Homelab` or `Bypass permissions · default`.
// Core's `inherit` decides the layer. This module only words it.
export function layerLabel(layer, { projectName = '', agentName = '', chats = false } = {}) {
  if (layer === 'thread') return 'this thread'
  if (layer === 'agent') return agentName ? `from agent ${agentName}` : 'from its agent'
  if (layer === 'project') {
    if (chats) return 'from Chats'
    return projectName ? `from project ${projectName}` : 'from its project'
  }
  return 'default'
}

// A value and its layer on one line.
export function sourceLine(value, layer, names) {
  return value ? `${value} · ${layerLabel(layer, names)}` : layerLabel(layer, names)
}

// The tools, MCP servers and skills an agent can use, as its own machine
// reports them through core's `agent.tool_catalog`. Core builds the catalog
// from that runtime's harness and extensions, so an agent on a paired machine
// lists that machine's tools, not this computer's.
import { agentRequest } from './agent-runtime.js'

export function emptyCatalog() {
  return { builtin: [], mcp: [], skills: [], loaded: false, error: '' }
}

// The catalog of the machine `runtime` names. A machine that does not answer
// the operation leaves the catalog empty with its reason.
export async function loadToolCatalog(tauri, runtime) {
  try {
    const answer = await agentRequest(tauri, runtime, 'agent.tool_catalog', {})
    return {
      builtin: (answer?.builtin ?? []).map((tool) => ({ id: tool.id, label: tool.label || tool.id, readOnly: !!tool.read_only })),
      mcp: (answer?.mcp ?? []).map((server) => ({ server: server.server, tools: (server.tools ?? []).map((tool) => tool.name), checkedAt: server.checked_at ?? null })),
      skills: (answer?.skills ?? []).map((skill) => ({ id: skill.id, name: skill.name || skill.id, description: skill.description ?? '' })),
      loaded: true,
      error: '',
    }
  } catch (failure) {
    return { ...emptyCatalog(), loaded: true, error: String(failure?.message ?? failure) }
  }
}

// The agent's `tools` after one tool turns on or off. Its choices over its
// project's tools stay.
export function toggleTool(agent, id, enabled) {
  const disabled = new Set(agent?.tools?.disabled ?? [])
  if (enabled) disabled.delete(id)
  else disabled.add(id)
  return { ...(agent?.tools ?? {}), disabled: [...disabled] }
}

const without = (list, id) => (list ?? []).filter((entry) => entry !== id)

// A project's tools after one row of core's `project_tools` turns on or off.
export function projectToolPolicy(policy, row, enabled) {
  const key = row.kind === 'builtin' ? 'disabledTools' : 'disabledExtensions'
  const next = { disabledTools: [...(policy?.disabledTools ?? [])], disabledExtensions: [...(policy?.disabledExtensions ?? [])] }
  next[key] = enabled ? without(next[key], row.id) : [...without(next[key], row.id), row.id]
  return next.disabledTools.length || next.disabledExtensions.length ? next : null
}

// The agent's `tools` after one row turns on or off. A choice that matches
// the project's drops the agent's own, so the row follows the project again.
export function agentToolChoice(agent, policy, row, enabled) {
  const tools = agent?.tools ?? {}
  const next = {
    ...tools,
    disabled: without(tools.disabled, row.id),
    disabled_extensions: without(tools.disabled_extensions, row.id),
    allowed: without(tools.allowed, row.id),
  }
  const projectOn = !(row.kind === 'builtin' ? policy?.disabledTools : policy?.disabledExtensions)?.includes(row.id)
  if (enabled !== projectOn) {
    if (enabled) next.allowed.push(row.id)
    else next[row.kind === 'builtin' ? 'disabled' : 'disabled_extensions'].push(row.id)
  }
  return next
}

// The rows of core's `project_tools` for a machine's catalog, a project's
// tools and, for an agent's card, the agent's choices.
export function projectTools(tauri, catalog, policy, agentTools = null) {
  return tauri.invoke('project_tools', { input: { catalog: catalog ?? {}, policy: policy ?? {}, ...(agentTools ? { agent: agentTools } : {}) } })
}

// What a tool row's kind reads as.
export const TOOL_KINDS = { builtin: 'Built-in tool', mcp: 'MCP server', plugin: 'Plugin', skill: 'Skill' }

// The skill ids the agent selects that its machine no longer has.
export function missingSkills(agent, catalog = emptyCatalog()) {
  const installed = new Set(catalog.skills.map((skill) => skill.id))
  return (agent?.skills ?? []).filter((id) => !installed.has(id))
}

// The tools one MCP server offers, by its name, and a line when the machine
// has not tested the server yet, so its tools are unknown.
export function serverTools(catalog, name) {
  const server = catalog?.mcp?.find((entry) => entry.server === name)
  if (!server) return { tools: [], note: '' }
  if (!server.tools.length) return { tools: [], note: server.checkedAt ? 'This server offers no tools.' : 'Test this server in Tools to list its tools.' }
  return { tools: server.tools, note: '' }
}

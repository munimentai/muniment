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

// The tool toggles: the machine's built-in tools, then any id the agent
// turns off that the catalog does not list, so it can be turned on again.
export function agentTools(agent, catalog = emptyCatalog()) {
  const listed = catalog.builtin.map((tool) => ({ ...tool, known: true }))
  const known = new Set(listed.map((tool) => tool.id))
  const extra = (agent?.tools?.disabled ?? []).filter((id) => !known.has(id)).map((id) => ({ id, label: id, readOnly: false, known: false }))
  return [...listed, ...extra]
}

// The agent's `tools` after one tool turns on or off.
export function toggleTool(agent, id, enabled) {
  const disabled = new Set(agent?.tools?.disabled ?? [])
  if (enabled) disabled.delete(id)
  else disabled.add(id)
  return { disabled: [...disabled] }
}

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
  if (!server.tools.length) return { tools: [], note: server.checkedAt ? 'This server offers no tools.' : 'Test this server in Extend to list its tools.' }
  return { tools: server.tools, note: '' }
}

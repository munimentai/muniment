// The tool ids an agent can turn off. No runtime operation lists the
// account's tools yet, so the list is the harness's built-in tools plus any
// id the agent already turns off.
export const BUILT_IN_TOOLS = [
  ['read', 'Read files'],
  ['write', 'Write files'],
  ['edit', 'Edit files'],
  ['ls', 'List folders'],
  ['find', 'Find files'],
  ['grep', 'Search file contents'],
  ['bash', 'Shell commands'],
  ['powershell', 'PowerShell commands'],
  ['codemode', 'Scripts'],
  ['bg_run', 'Background tasks'],
  ['web_search', 'Web search'],
  ['fetch_content', 'Fetch web pages'],
]

export function agentTools(agent) {
  const known = new Set(BUILT_IN_TOOLS.map(([id]) => id))
  const extra = (agent?.tools?.disabled ?? []).filter((id) => !known.has(id)).map((id) => [id, id])
  return [...BUILT_IN_TOOLS, ...extra]
}

// The agent's `tools` after one tool turns on or off.
export function toggleTool(agent, id, enabled) {
  const disabled = new Set(agent?.tools?.disabled ?? [])
  if (enabled) disabled.delete(id)
  else disabled.add(id)
  return { disabled: [...disabled] }
}

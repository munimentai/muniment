// A stand-in for core's `sidebar_arrange` in window tests and probe pages
// that do not test the arrangement. It keeps every thread in its project and
// makes each thread in no project its own row, in the order given: pinned
// rows first, a project where its first thread falls, then projects with no
// thread by name. Running threads say so, archived ones fold, and nothing
// else carries a status. Core's own tests and the projects probe cover the
// real arrangement.
export function arrangeStandIn(input = {}) {
  const entry = (id, name, kind, owner) => ({ id, name, kind, triggers: '', agent_ids: [], owner: owner ?? undefined, updated_at: '', status: 'read', status_label: '', pinned: false, threads: [] })
  const projects = new Map()
  for (const project of input.projects ?? []) projects.set(project.id, entry(project.id, project.name, 'project', project.owner))
  const agents = new Map((input.agents ?? []).map((agent) => [agent.id, agent]))
  const agentThreads = { ...(input.agent_threads ?? {}) }
  for (const agent of input.agents ?? []) {
    if (agent.home_thread_id) agentThreads[agent.home_thread_id] = agent.id
    const id = agent.project_id && projects.has(agent.project_id) ? agent.project_id : `agent:${agent.id}`
    if (!projects.has(id)) projects.set(id, entry(id, agent.name, 'agent'))
    projects.get(id).agent_ids.push(agent.id)
  }
  const pinned = new Set(input.pinned ?? [])
  const archived = new Set(input.archived ?? [])
  const working = new Set(input.working ?? [])
  const query = (input.query ?? '').trim().toLowerCase()
  const sidebar = { projects: [], archived: [], results: [] }
  const listed = []
  const seen = new Set()
  const homes = (input.agents ?? []).filter((agent) => agent.home_thread_id && !(input.threads ?? []).some((thread) => thread.id === agent.home_thread_id)).map((agent) => ({ id: agent.home_thread_id, title: '', updated_at: '' }))
  for (const thread of [...(input.threads ?? []), ...homes]) {
    if (!thread.id || seen.has(thread.id)) continue
    seen.add(thread.id)
    const agent = agents.get(agentThreads[thread.id])
    const projectId = agent ? (agent.project_id && projects.has(agent.project_id) ? agent.project_id : `agent:${agent.id}`)
      : projects.has(input.assignments?.[thread.id]) ? input.assignments[thread.id] : null
    const status = working.has(thread.id) ? 'working' : 'read'
    const row = {
      thread_id: thread.id, title: thread.title || agent?.name || 'New thread', project_id: projectId ?? '', project_name: projectId ? projects.get(projectId).name : '',
      machine: thread.machine ?? undefined, updated_at: thread.updated_at, status, status_label: status === 'working' ? 'Working' : '',
      agent_id: agent?.id, home: agent?.home_thread_id === thread.id, pinned: pinned.has(thread.id),
    }
    if (query && (row.title.toLowerCase().includes(query) || row.project_name.toLowerCase().includes(query))) sidebar.results.push(row)
    if (archived.has(thread.id)) { sidebar.archived.push(row); continue }
    const owner = projectId ? projects.get(projectId) : { ...entry(thread.id, row.title, 'thread'), updated_at: row.updated_at }
    owner.threads.push(row)
    if (row.pinned) owner.pinned = true
    if (status === 'working') { owner.status = 'working'; owner.status_label = 'Working' }
    if (!owner.updated_at) owner.updated_at = row.updated_at
    if (!listed.includes(owner)) listed.push(owner)
  }
  const idle = [...projects.values()].filter((project) => !listed.includes(project)).sort((a, b) => a.name.localeCompare(b.name))
  sidebar.projects = [...listed.filter((project) => project.pinned), ...listed.filter((project) => !project.pinned), ...idle]
  return sidebar
}

// A stand-in for core's `project_tools` in window tests and probe pages:
// each catalog tool with whether the project, and an agent's choices over
// it, allow it, and the layer that decides. Core's tests cover the real one.
export function toolsStandIn({ catalog = {}, policy = {}, agent = null } = {}) {
  const groups = [['builtin', catalog.builtin], ['mcp', catalog.mcp], ['plugin', catalog.plugins], ['skill', catalog.skills]]
  return groups.flatMap(([kind, entries]) => (entries ?? []).filter((entry) => entry.id).map((entry) => {
    const off = (kind === 'builtin' ? policy?.disabledTools : policy?.disabledExtensions)?.includes(entry.id)
    const agentOff = (kind === 'builtin' ? agent?.disabled : agent?.disabled_extensions)?.includes(entry.id)
    const agentOn = agent?.allowed?.includes(entry.id)
    const [allowed, layer] = agentOn ? [true, 'agent'] : agentOff ? [false, 'agent'] : off ? [false, 'project'] : [true, 'default']
    return { id: entry.id, kind, label: entry.label || entry.name || entry.server || entry.id, allowed, layer }
  }))
}

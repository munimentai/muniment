// The projects probe: several projects, an agent on a schedule, one on a
// webhook in its project, threads that need the user, threads at work and
// settled threads. The sidebar shows core's own arrangement of
// `core/protocol-fixtures/sidebar/input.json`, which core's tests keep in
// `arranged.json`, with every time moved so the fixture's now is this moment.
// `?state=settled` opens Show settled, `triggers` filters Projects to the
// projects with triggers, `search` searches for "re", and `settings` opens
// Homelab's settings with its webhook agent.
const FIXTURE_NOW = Date.parse('2026-10-18T13:00:00Z')
const params = typeof location === 'undefined' ? new URLSearchParams() : new URLSearchParams(location.search)
export const projectsState = params.get('state') ?? 'sidebar'

const shift = (time) => time ? new Date(Date.parse(time) + Date.now() - FIXTURE_NOW).toISOString() : time

let input = null
let arranged = null
async function load() {
  input ??= await (await fetch('../../core/protocol-fixtures/sidebar/input.json')).json()
  arranged ??= await (await fetch('../../core/protocol-fixtures/sidebar/arranged.json')).json()
}

// Core's arrangement with its times moved to now. The search state answers
// the search the window sends with the rows whose titles match.
export async function projectsArrangement(request) {
  await load()
  const rows = (list) => list.map((row) => ({ ...row, updated_at: shift(row.updated_at) }))
  const sidebar = Object.fromEntries(Object.entries(arranged).map(([key, value]) => [key, key === 'projects' ? value.map((project) => ({ ...project, threads: rows(project.threads) })) : rows(value)]))
  if (request?.filter === 'triggers') sidebar.projects = sidebar.projects.filter((project) => project.kind !== 'chats' && project.agent_ids.length)
  const query = request?.query?.trim().toLowerCase()
  if (query) sidebar.results = [...sidebar.needs_you, ...sidebar.working, ...sidebar.pinned, ...sidebar.recent, ...sidebar.settled].filter((row) => row.title.toLowerCase().includes(query))
  return sidebar
}

export const projectsSummaries = () => [
  'Clean the build folder', 'Release notes', 'Fix sync bug', 'Reindex search', 'Roadmap', 'Trip ideas', 'Pricing watch', 'Dinner recipes', 'Deploy runs', 'Lease renewal', 'Vendor audit', 'Budget notes', 'Kitchen quotes',
].map((title, index) => ({ threadId: ['t-clean', 't-branch', 't-sync', 't-index', 't-roadmap', 't-trip', 't-price', 't-dinner', 't-deploy', 't-lease', 't-vendor', 't-budget', 't-kitchen'][index], title, updatedAt: shift('2026-10-18T12:00:00Z') }))

export const projectsCatalog = {
  projects: { 'p-muniment': 'Muniment', 'p-homelab': 'Homelab', 'p-garden': 'Garden' },
  threads: { 't-clean': 'p-muniment', 't-sync': 'p-muniment', 't-index': 'p-muniment', 't-roadmap': 'p-muniment', 't-branch': 'p-homelab', 't-vendor': 'p-homelab', 't-kitchen': 'p-garden' },
  settings: { 'p-homelab': { permissionMode: 'manual' } },
}

// The two agents of the fixture, as `agent.list` answers them.
export function projectsAgents() {
  const agent = (id, name, label, projectId, routine) => ({
    id, name, label, instructions: `${label}.`, avatar: { seed: id, style: 'muniment-v2' }, template: null, projectId, schedule: null,
    primary_runtime: null, paused_reason: null, routines: [routine],
  })
  return {
    agents: [
      agent('pricing-watch', 'Pricing watch', 'Watch competitor prices each morning', null, { id: 'morning', name: 'Morning check', enabled: true, created_from: 'user', prompt: '', trigger: { kind: 'cron', expr: '0 9 * * *', tz: 'America/New_York' }, last_fired_at: shift('2026-10-18T09:00:00Z'), last_result: { run_id: 'run-price', status: 'failed', silent: false } }),
      agent('deploy-bot', 'Deploy bot', 'Deploy the homelab stack when a push lands', 'p-homelab', { id: 'push', name: 'Push landed', enabled: true, created_from: 'user', prompt: 'Deploy the change.', trigger: { kind: 'webhook', hook_id: '4f1c2b7a9d0e4c55', preset: 'forgejo' }, last_fired_at: shift('2026-10-18T08:15:00Z'), last_result: { run_id: 'run-deploy', status: 'completed', silent: false } }),
    ],
    primaryThreads: { 'pricing-watch': 't-price', 'deploy-bot': 't-deploy' },
  }
}

export const projectsSettings = (projectId) => projectId === 'p-homelab'
  ? { permissionMode: 'manual', instructions: 'Use the Ansible playbooks. Never edit the hosts by hand.' }
  : projectId === 'chats' ? {} : { instructions: '' }

const button = (root, name) => [...(root?.querySelectorAll('button') ?? [])].find((entry) => entry.getAttribute('aria-label') === name || entry.textContent.trim() === name)

// Moves the page to the state's screen, one step per render.
export function advanceProjects() {
  const sidebar = document.querySelector('#sidebar')
  if (!sidebar?.querySelector('[aria-label="Needs you"]')) return
  if (projectsState === 'settled') {
    const settled = sidebar.querySelector('[aria-label="Settled threads"]')
    if (settled) return settled.scrollIntoView({ block: 'end' })
    return button(sidebar, 'Show settled (5)')?.click()
  }
  if (projectsState === 'triggers') {
    const filter = button(sidebar, 'Projects with triggers')
    if (filter?.getAttribute('aria-pressed') === 'false') filter.click()
    return
  }
  if (projectsState === 'search') {
    const search = sidebar.querySelector('input[aria-label="Search threads"]')
    if (search && search.value !== 're') { search.value = 're'; search.dispatchEvent(new Event('input', { bubbles: true })) }
    return
  }
  if (projectsState === 'settings' && !document.querySelector('[aria-label="Project settings"]')) {
    const menu = document.querySelector('[role="menu"][aria-label="Homelab actions"]')
    if (menu) return button(menu, 'Settings')?.click()
    return button(sidebar, 'Homelab actions')?.click()
  }
}

export function projectsRendered() {
  const sidebar = document.querySelector('#sidebar')
  if (!sidebar) return false
  const sections = ['Needs you', 'Working', 'Pinned', 'Projects', 'Recent'].every((name) => sidebar.querySelector(`[aria-label="${name}"]`))
  if (projectsState === 'search') return !!sidebar.querySelector('[aria-label="Matching threads"] .thread-row')
  if (!sections) return false
  if (projectsState === 'settled') return !!sidebar.querySelector('[aria-label="Settled threads"] .thread-row')
  if (projectsState === 'triggers') return sidebar.querySelectorAll('[aria-label="Projects"] .project-title').length === 2
  if (projectsState === 'settings') return !!document.querySelector('[aria-label="Project settings"] .agent-card') && !!document.querySelector('[aria-label="Project settings"] textarea')?.value
  return sidebar.textContent.includes('every day 9:00') && sidebar.textContent.includes('webhook') && sidebar.textContent.includes('Approve')
}

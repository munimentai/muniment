// The projects probe: several projects, an agent on a schedule whose thread
// is a row of its own, one on a webhook in its project, threads in no project, threads that need the user,
// threads at work and settled threads. The sidebar shows core's own
// arrangement of `core/protocol-fixtures/sidebar/input.json`, which core's
// tests keep in `arranged.json`, with every time moved so the fixture's now
// is this moment. `?state=search` searches for "re", and `settings` opens
// Homelab's settings with its webhook agent and each default's source.
// `palette` opens the
// command palette and `palette-search` searches it for "re". `move` opens
// Move to project for Trip ideas, and `model` opens the model picker on
// Release notes, which runs Homelab's model.
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
  const sidebar = Object.fromEntries(Object.entries(arranged).map(([key, value]) => [key, key === 'projects' ? value.map((project) => ({ ...project, updated_at: shift(project.updated_at), threads: rows(project.threads) })) : rows(value)]))
  const query = request?.query?.trim().toLowerCase()
  if (query) sidebar.results = [...sidebar.projects.flatMap((project) => project.threads), ...sidebar.archived].filter((row) => row.title.toLowerCase().includes(query))
  return sidebar
}

export const projectsSummaries = () => [
  'Clean the build folder', 'Release notes', 'Fix sync bug', 'Reindex search', 'Roadmap', 'Trip ideas', 'Pricing watch', 'Dinner recipes', 'Deploy runs', 'Lease renewal', 'Vendor audit', 'Budget notes', 'Kitchen quotes',
].map((title, index) => ({ threadId: ['t-clean', 't-branch', 't-sync', 't-index', 't-roadmap', 't-trip', 't-price', 't-dinner', 't-deploy', 't-lease', 't-vendor', 't-budget', 't-kitchen'][index], title, updatedAt: shift('2026-10-18T12:00:00Z') }))

const HOMELAB_MODEL = { provider: 'google', model: 'gemini-3-pro' }
export const projectsCatalog = {
  projects: { 'p-muniment': 'Muniment', 'p-homelab': 'Homelab', 'p-garden': 'Garden' },
  threads: { 't-clean': 'p-muniment', 't-sync': 'p-muniment', 't-index': 'p-muniment', 't-roadmap': 'p-muniment', 't-branch': 'p-homelab', 't-vendor': 'p-homelab', 't-kitchen': 'p-garden' },
  settings: { 'p-homelab': { permissionMode: 'manual', model: HOMELAB_MODEL } },
}

// Each thread's model, as core's `thread_settings` answers it. Homelab's
// threads started with its model; every other thread runs the default.
const threadModels = {
  't-branch': { model: HOMELAB_MODEL, seed: { project_id: 'p-homelab', model: HOMELAB_MODEL, permission_mode: 'manual' } },
  't-vendor': { model: HOMELAB_MODEL, seed: { project_id: 'p-homelab', model: HOMELAB_MODEL, permission_mode: 'manual' } },
}
const DEFAULT_MODEL = { provider: 'ollama', model: 'llama3.2:3b' }
const same = (a, b) => !!a && !!b && a.provider === b.provider && a.model === b.model
export function projectsThreadSettings(threadId, change = null) {
  const own = threadModels[threadId] ??= {}
  if (change?.seed) { own.seed = change.seed; own.model = change.seed.model ?? null }
  if (change?.model) own.model = change.model
  if (change?.reset) own.model = own.seed?.model ?? null
  const seeded = same(own.model, own.seed?.model)
  return {
    thread_id: threadId,
    model: own.model ?? DEFAULT_MODEL,
    layer: own.model ? (seeded ? 'project' : 'thread') : 'default',
    ...(seeded ? { project_id: own.seed.project_id } : {}),
    ...(own.seed ? { seed: own.seed } : {}),
  }
}

// Core's palette ranking of `core/protocol-fixtures/palette/input.json`,
// which core's tests keep in `ranked.json`, for the queries it holds.
let ranked = null
export async function projectsPalette(request) {
  ranked ??= await (await fetch('../../core/protocol-fixtures/palette/ranked.json')).json()
  return structuredClone(ranked[request?.query ?? ''] ?? ranked[''])
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
  ? { ...structuredClone(projectsCatalog.settings['p-homelab']), instructions: 'Use the Ansible playbooks. Never edit the hosts by hand.' }
  : { instructions: '' }

const button = (root, name) => [...(root?.querySelectorAll('button') ?? [])].find((entry) => entry.getAttribute('aria-label') === name || entry.textContent.trim() === name)

// Moves the page to the state's screen, one step per render.
export function advanceProjects() {
  const sidebar = document.querySelector('#sidebar')
  if (!sidebar?.querySelector('[aria-label="Projects list"] .project-title')) return
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
  if (projectsState.startsWith('palette')) {
    const field = document.querySelector('[role="dialog"][aria-label="Search"] input')
    if (!field) {
      const mac = navigator.platform.startsWith('Mac')
      return document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: mac, ctrlKey: !mac, bubbles: true }))
    }
    if (projectsState === 'palette-search' && field.value !== 're') { field.value = 're'; field.dispatchEvent(new Event('input', { bubbles: true })) }
    return
  }
  if (projectsState === 'move' && !document.querySelector('[aria-labelledby="move-project-title"]')) {
    const menu = document.querySelector('.thread-menu')
    if (menu) return button(menu, 'Move to project')?.click()
    return sidebar.querySelector('button[aria-label="Actions for Trip ideas"]')?.click()
  }
  if (projectsState === 'model') {
    const homelab = sidebar.querySelector('.project-title[aria-label^="Project Homelab"]')
    if (homelab?.getAttribute('aria-expanded') === 'false' && !sidebar.querySelector('[aria-label="Homelab threads"]')) return homelab.click()
    const row = sidebar.querySelector('.thread-row[data-thread-id="t-branch"]')
    if (row && row.tagName === 'BUTTON' && !row.getAttribute('aria-current')) return row.click()
    const chip = document.querySelector('[data-menu="model"][aria-expanded="false"]')
    if (chip && document.querySelector('.thread-row.active-thread[data-thread-id="t-branch"]')) chip.click()
  }
}

export function projectsRendered() {
  const sidebar = document.querySelector('#sidebar')
  if (!sidebar) return false
  if (projectsState === 'search') return !!sidebar.querySelector('[aria-label="Matching threads"] .thread-row')
  if (!sidebar.querySelector('[aria-label="Projects list"] .project-title')) return false
  const settings = document.querySelector('[aria-label="Project settings"]')
  if (projectsState === 'settings') return !!settings?.querySelector('.agent-card') && !!settings.querySelector('textarea')?.value && settings.textContent.includes('Gemini 3 Pro · this project')
  if (projectsState === 'palette') return !!document.querySelector('[role="dialog"][aria-label="Search"] .palette-row') && document.querySelector('[role="dialog"][aria-label="Search"]').textContent.includes('Needs you')
  if (projectsState === 'palette-search') return document.querySelector('[role="dialog"][aria-label="Search"] input')?.value === 're' && !!document.querySelector('[role="dialog"][aria-label="Search"]')?.textContent.includes('Reindex search')
  if (projectsState === 'move') return !!document.querySelector('[aria-labelledby="move-project-title"] button[aria-current="true"]')
  if (projectsState === 'model') return !!document.querySelector('.picker-source')?.textContent.includes('from project Homelab')
  // The scheduled agent's thread is a row that names the agent, and only a project shows triggers.
  return !!sidebar.querySelector('.thread-row .row-agent') && !sidebar.textContent.includes('every day 9:00') && sidebar.textContent.includes('webhook') && sidebar.textContent.includes('Approve')
}

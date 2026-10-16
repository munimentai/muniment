// Agents on three machines for the probe: this computer, a studio desktop
// and a build box container. `handleAgentRequest` answers `agent_request`
// with the core attach operations' shapes, as the protocol fixtures show.

export const STUDIO = '018f0000-0000-7000-8000-000000000042'
export const BOX = '018f0000-0000-7000-8000-000000000043'
const SCOUT = '018f0000-0000-4000-8000-0000000000a1'
const INBOX = '018f0000-0000-4000-8000-0000000000a2'
const EDITOR = '018f0000-0000-4000-8000-0000000000a3'
const TRIAGE = '018f0000-0000-4000-8000-0000000000a4'

const at = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString()
const clone = (value) => structuredClone(value)

function run(id, agentId, minutesAgo, status, trigger, extra = {}) {
  const updated = at(minutesAgo)
  const created = at(minutesAgo + (extra.minutes ?? 2))
  delete extra.minutes
  return { id, agent_id: agentId, thread_id: `thread-${agentId.slice(-2)}`, trigger, status, attempt: 1, hop: 0, lease_owner: null, lease_expires_at: null, origin_run_id: null, created_at: created, updated_at: updated, result_summary: null, silent: false, ...extra }
}

function seeded() {
  const zone = 'America/New_York'
  const scout = {
    id: SCOUT, name: 'Lead scout', label: 'Researcher', instructions: 'Find new leads in the CRM each weekday morning, check them against the target list and summarize the promising ones in leads.md.',
    avatar: { seed: 'scout', style: 'muniment-v2' }, template: null, projectId: null, schedule: { cadence: 'weekdays', enabled: true, time: '09:00', weekday: 0 },
    model: { provider: 'ollama', id: 'qwen3:8b', thinking: 'medium' }, tools: { disabled: ['powershell'] }, skills: ['review'],
    permissions: {
      mode: 'ask',
      always_allow: [{ request_kind: 'command', resource: { type: 'command', arguments: ['bash', 'git status'], working_directory: '/Users/alice/Documents/muniment/agents/Lead-scout' } }],
      ask_first: [{ request_kind: 'tool', resource: { type: 'tool', name: 'mcp__crm__delete_lead', input: { id: 'lead-1' } } }],
    },
    primary_runtime: null, messaging: { allow: [INBOX] }, paused_reason: null,
    routines: [
      { id: 'schedule', name: 'Weekday briefing', enabled: true, created_from: 'user', prompt: '', trigger: { kind: 'cron', expr: '0 9 * * 1-5', tz: zone }, last_fired_at: at(190), last_result: { run_id: 'run-1', status: 'completed', silent: false } },
      { id: 'build-hook', name: 'Build finished', enabled: true, created_from: 'user', prompt: 'Summarize the build result.', trigger: { kind: 'webhook', hook_id: '4f1c2b7a9d0e4c55b6a1f2e3d4c5b6a7', preset: 'forgejo' }, last_fired_at: at(1500), last_result: { run_id: 'run-9', status: 'failed', silent: false } },
      { id: 'new-leads', name: 'New leads', enabled: true, created_from: 'instructions', prompt: 'Pull the new leads and draft a summary.', trigger: { kind: 'mcp', server: 'crm', tool: 'search_leads', args: { status: 'new' }, every: { expr: '*/30 * * * *', tz: zone }, condition: { all: [{ op: 'gt', path: '/total', value: 0 }, { any: [{ op: 'eq', path: '/items/0/region', value: 'west' }, { op: 'changed', path: '/items' }] }] } }, last_fired_at: at(20), last_result: { run_id: 'run-3', status: 'completed', silent: true } },
    ],
  }
  const inbox = {
    id: INBOX, name: 'Inbox triage', label: 'Assistant', instructions: 'Every weekday at 8am send me a digest of new mail. If an urgent message arrives, escalate it at once.',
    avatar: { seed: 'inbox', style: 'muniment-v2' }, template: null, projectId: null, schedule: null, primary_runtime: null, paused_reason: null,
    routines: [
      { id: 'digest', name: 'Morning digest', enabled: false, created_from: 'instructions', prompt: 'Send a digest of new mail.', trigger: { kind: 'cron', expr: '0 8 * * 1-5', tz: zone }, last_fired_at: null, last_result: null },
      { id: 'urgent', name: 'Escalate urgent mail', enabled: false, created_from: 'instructions', prompt: 'Escalate the urgent message.', trigger: { kind: 'mcp', server: 'mail', tool: 'list_unread', args: {}, every: { expr: '*/5 * * * *', tz: zone }, condition: { all: [{ op: 'gt', path: '/unread', value: 0 }, { any: [{ op: 'eq', path: '/items/0/priority', value: 'urgent' }, { op: 'changed', path: '/items' }] }] } }, last_fired_at: null, last_result: null },
    ],
  }
  const editor = {
    id: EDITOR, name: 'Release editor', label: 'Editor', instructions: 'Draft the release notes from merged changes every Friday afternoon.',
    avatar: { seed: 'editor', style: 'muniment-v2' }, template: null, projectId: null, schedule: null, primary_runtime: null, paused_reason: null,
    routines: [{ id: 'notes', name: 'Friday notes', enabled: true, created_from: 'user', prompt: 'Draft this week’s release notes.', trigger: { kind: 'cron', expr: '0 16 * * 5', tz: 'Europe/Berlin' }, last_fired_at: at(4000), last_result: { run_id: 'run-20', status: 'completed', silent: false } }],
  }
  const triage = {
    id: TRIAGE, name: 'Nightly test triage', label: 'Test engineer', instructions: 'Read the nightly test results and file the failures.',
    avatar: { seed: 'triage', style: 'muniment-v2' }, template: null, projectId: null, schedule: null, primary_runtime: null, paused_reason: null,
    routines: [
      { id: 'nightly', name: 'After the nightly', enabled: true, created_from: 'user', prompt: 'Triage the failures.', trigger: { kind: 'cron', expr: '0 2 * * *', tz: 'UTC' }, last_fired_at: at(600), last_result: { run_id: 'run-30', status: 'waiting_input', silent: false } },
      { id: 'ci', name: 'CI failed', enabled: false, created_from: 'user', prompt: 'Look at the failed job.', trigger: { kind: 'webhook', hook_id: '9a0b1c2d3e4f40516273849506172839', preset: 'github' }, last_fired_at: at(9000), last_result: { run_id: 'run-31', status: 'completed', silent: false } },
    ],
  }
  return {
    local: {
      agents: [scout, inbox], primaryThreads: { [INBOX]: 'thread-a2' },
      runs: [
        run('run-2', SCOUT, 0, 'running', { kind: 'manual' }, { minutes: 1 }),
        run('run-3', SCOUT, 20, 'completed', { kind: 'mcp', routine_id: 'new-leads' }, { silent: true, minutes: 0.4 }),
        run('run-1', SCOUT, 190, 'completed', { kind: 'cron', routine_id: 'schedule' }, { result_summary: 'Three new leads. The summary is in leads.md.', minutes: 3 }),
        run('run-4', INBOX, 45, 'queued', { kind: 'manual' }, { minutes: 0 }),
        run('run-9', SCOUT, 1500, 'failed', { kind: 'webhook', routine_id: 'build-hook' }, { error: 'The build log was not in the webhook payload.', minutes: 1 }),
      ],
    },
    [STUDIO]: {
      agents: [editor], primaryThreads: { [EDITOR]: 'thread-a3' },
      runs: [run('run-20', EDITOR, 4000, 'completed', { kind: 'cron', routine_id: 'notes' }, { result_summary: 'Release notes drafted in notes/v0.4.md.', minutes: 6 })],
    },
    [BOX]: {
      agents: [triage], primaryThreads: { [TRIAGE]: 'thread-a4' },
      runs: [
        run('run-30', TRIAGE, 600, 'waiting_input', { kind: 'cron', routine_id: 'nightly' }, { waiting_since: at(598), minutes: 2 }),
        run('run-31', TRIAGE, 9000, 'completed', { kind: 'webhook', routine_id: 'ci' }, { result_summary: 'Filed two failures.', minutes: 4 }),
        run('run-32', TRIAGE, 9200, 'cancelled', { kind: 'manual' }, { minutes: 1 }),
      ],
    },
  }
}

export function probeAgentStores(seed) {
  return seed ? seeded() : { local: { agents: [], primaryThreads: {}, runs: [] } }
}

// The installed extensions the agent profile offers.
export const probeExtensions = {
  items: [
    { id: 'review', kind: 'skill', name: 'Code review' },
    { id: 'summarize', kind: 'skill', name: 'Summaries' },
    { id: 'crm-server', kind: 'mcp', name: 'crm' },
    { id: 'mail-server', kind: 'mcp', name: 'mail' },
  ],
}

const newestFirst = (a, b) => b.updated_at.localeCompare(a.updated_at)

export function handleAgentRequest(stores, { runtime, operation, body = {} }) {
  const store = stores[runtime || 'local']
  if (!store) throw 'That machine is offline. Try again when it answers.'
  const find = (id) => {
    const agent = store.agents.find((entry) => entry.id === id)
    if (!agent) throw 'The agent was not found.'
    return agent
  }
  const latest = () => Object.fromEntries(store.agents.flatMap((agent) => {
    const newest = store.runs.filter((entry) => entry.agent_id === agent.id).sort(newestFirst)[0]
    return newest ? [[agent.id, clone(newest)]] : []
  }))
  if (operation === 'agent.list') return { agents: clone(store.agents), primary_threads: { ...store.primaryThreads }, latest_runs: latest(), errors: [] }
  if (operation === 'agent.read') return { agent: clone(find(body.id)), primary_thread_id: store.primaryThreads[body.id] ?? null }
  if (operation === 'agent.save') {
    const agent = clone({ ...body.agent, id: body.agent.id || crypto.randomUUID() })
    store.agents = [...store.agents.filter((entry) => entry.id !== agent.id), agent]
    return { agent: clone(agent) }
  }
  if (operation === 'agent.delete') { store.agents = store.agents.filter((entry) => entry.id !== body.id); return {} }
  if (operation === 'agent.run_now') {
    const queued = run(crypto.randomUUID(), body.id, 0, 'queued', { kind: 'manual' }, { minutes: 0 })
    store.runs.unshift(queued)
    return { run: clone(queued) }
  }
  if (operation === 'agent.runs') {
    const runs = store.runs.filter((entry) => !body.agent_id || entry.agent_id === body.agent_id).sort(newestFirst).slice(0, body.limit ?? 50)
    return { runs: clone(runs), next_cursor: null }
  }
  if (operation === 'routine.list') return { routines: clone(find(body.agent_id).routines) }
  if (operation === 'routine.save') {
    const agent = find(body.agent_id)
    const routine = clone({ ...body.routine, id: body.routine.id || crypto.randomUUID() })
    agent.routines = agent.routines.some((entry) => entry.id === routine.id)
      ? agent.routines.map((entry) => entry.id === routine.id ? routine : entry)
      : [...agent.routines, routine]
    return { routine: clone(routine) }
  }
  if (operation === 'routine.delete') {
    const agent = find(body.agent_id)
    agent.routines = agent.routines.filter((entry) => entry.id !== body.routine_id)
    return {}
  }
  throw 'The agent action is unknown.'
}

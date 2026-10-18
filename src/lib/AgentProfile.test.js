import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentProfile from './AgentProfile.svelte'
afterEach(cleanup)

// Core's `agent.tool_catalog` from the agent's machine, as the window maps it.
const catalog = {
  builtin: [{ id: 'read', label: 'Read files', readOnly: true }, { id: 'bash', label: 'Shell commands', readOnly: false }],
  mcp: [{ server: 'crm', tools: ['search_leads', 'get_lead'], checkedAt: '2026-01-01T08:55:00Z' }, { server: 'calendar', tools: [], checkedAt: null }],
  skills: [{ id: 'review', name: 'Code review', description: 'Review a draft.' }],
  loaded: true,
  error: '',
}
const agent = { id: 'scout', name: 'Scout', instructions: 'Find sources.', projectId: 'p', schedule: null, runtimeId: 'local', routines: [], tools: { disabled: ['powershell'] }, skills: [], permissions: { mode: 'manual', always_allow: [], ask_first: [] } }

// Core's `permission.modes` on the agent's machine, with bypass off.
const modes = {
  modes: [
    { id: 'manual', title: 'Ask before changes', description: 'Reads run. Edits ask first.', available: true },
    { id: 'plan', title: 'Plan first', description: 'Plans first.', available: true },
    { id: 'dont_ask', title: "Don't ask", description: 'Denies the rest.', available: true },
    { id: 'bypass', title: 'Bypass permissions', description: 'Runs everything.', available: false, reason: 'Bypass is off.' },
  ],
  default_thread_mode: 'manual',
  default_agent_mode: 'plan',
}

// Answers `agent_request` from one stored agent per machine and records each request.
function machine(stored = agent, extra = {}) {
  const requests = []
  const invoke = vi.fn(async (command, args) => {
    if (command !== 'agent_request') return extra[command]?.(args) ?? null
    requests.push(args)
    if (args.operation === 'agent.read') return { agent: structuredClone(stored), primary_thread_id: null }
    if (args.operation === 'agent.save') return { agent: args.body.agent }
    if (args.operation === 'permission.modes') return structuredClone(modes)
    if (args.operation === 'agent.run_now') return { run: { id: 'run-new', agent_id: stored.id, status: 'queued', trigger: { kind: 'manual' } } }
    if (args.operation === 'agent.routine_proposals') return { proposals: stored.routines.filter((routine) => routine.proposed) }
    if (args.operation === 'agent.export') return { format: args.body.format, file_name: `${stored.name}.${args.body.format}.md`, content: '# Export' }
    return {}
  })
  // The proposal cards, the failed runs and the mode list read core's live lists on their own.
  const changes = () => requests.filter((request) => !['agent.routine_proposals', 'agent.replays', 'permission.modes'].includes(request.operation))
  return { invoke, requests, changes, saved: () => requests.filter((request) => request.operation === 'agent.save').at(-1)?.body.agent }
}

it('saves one field over the agent its machine holds and leaves its routines alone', async () => {
  const { invoke, changes, saved } = machine({ ...agent, instructions: 'Manually edited on disk.' })
  const onchange = vi.fn()
  render(AgentProfile, { agent, tauri: { invoke }, projects: [['p', 'Research']], onchange, onclose: vi.fn() })
  expect(screen.getByText('Research')).toBeInTheDocument()
  expect(screen.queryByText(/Schedule/)).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Edit name' }))
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Cancel this' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  expect(changes()).toEqual([])
  expect(invoke.mock.calls.every(([command]) => command === 'agent_request')).toBe(true)
  await fireEvent.click(screen.getByRole('button', { name: 'Edit name' }))
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Research scout' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save', exact: true }))
  await waitFor(() => expect(onchange).toHaveBeenCalled())
  expect(changes()[0]).toEqual({ runtime: 'local', operation: 'agent.read', body: { id: 'scout' } })
  expect(saved()).toEqual({ ...agent, name: 'Research scout', instructions: 'Manually edited on disk.', schedule: null })
})

it('keeps a failed edit for retry and names the failure', async () => {
  let fail = true
  const invoke = vi.fn(async (command, args) => {
    if (args.operation === 'agent.read') return { agent }
    if (args.operation === 'agent.save' && fail) throw new Error('The machine refused the agent request. Check the fields and try again.')
    return { agent }
  })
  render(AgentProfile, { agent, tauri: { invoke }, onchange: vi.fn(), onclose: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: 'Edit job title' }))
  await fireEvent.input(screen.getByRole('textbox', { name: 'Job title' }), { target: { value: 'Analyst' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save', exact: true }))
  expect(await screen.findByRole('alert')).toHaveTextContent('The machine refused the agent request.')
  expect(screen.getByRole('textbox', { name: 'Job title' })).toHaveValue('Analyst')
  fail = false
  await fireEvent.click(screen.getByRole('button', { name: 'Save', exact: true }))
  await waitFor(() => expect(screen.queryByRole('textbox', { name: 'Job title' })).not.toBeInTheDocument())
})

it('turns skills on and off, sets the mode from core, edits permission rules and the messaging list', async () => {
  const { invoke, saved } = machine()
  const others = [{ id: 'editor', name: 'Editor', runtimeId: 'box' }]
  render(AgentProfile, { agent, tauri: { invoke }, agents: [agent, ...others], catalog, onchange: vi.fn() })
  // The tool switches are gone. The deprecated list waits under Advanced.
  expect(screen.queryByRole('switch', { name: 'Shell commands on' })).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('checkbox', { name: /^Code review/ }))
  await waitFor(() => expect(saved().skills).toEqual(['review']))

  const mode = await screen.findByRole('combobox', { name: 'Permission mode' })
  expect([...mode.options].map((option) => option.textContent)).toEqual(['Default (Plan first)', 'Ask before changes', 'Plan first', "Don't ask", 'Bypass permissions'])
  expect(mode.options[4]).toBeDisabled()
  await fireEvent.change(mode, { target: { value: 'dont_ask' } })
  await waitFor(() => expect(saved().permissions.mode).toBe('dont_ask'))
  // The default sends no mode, so the agent follows the machine's default.
  await fireEvent.change(mode, { target: { value: '' } })
  await waitFor(() => expect(saved().permissions).toEqual({ always_allow: [], ask_first: [] }))

  await fireEvent.click(screen.getByRole('button', { name: 'Add rule' }))
  const form = screen.getByRole('form', { name: 'New permission rule' })
  expect([...within(form).getByRole('combobox', { name: 'List' }).options].map((option) => option.textContent)).toEqual(['Deny', 'Ask first', 'Always allow'])
  await fireEvent.change(within(form).getByRole('combobox', { name: 'List' }), { target: { value: 'deny' } })
  await fireEvent.change(within(form).getByRole('combobox', { name: 'Kind' }), { target: { value: 'tool' } })
  await fireEvent.input(within(form).getByRole('textbox', { name: 'Tool' }), { target: { value: 'mcp__crm__delete_lead' } })
  await fireEvent.input(within(form).getByRole('textbox', { name: 'Input' }), { target: { value: '{"id":"lead-1"}' } })
  await fireEvent.click(within(form).getByRole('button', { name: 'Add rule' }))
  await waitFor(() => expect(saved().permissions.deny).toEqual([{ request_kind: 'tool', resource: { type: 'tool', name: 'mcp__crm__delete_lead', input: { id: 'lead-1' } } }]))

  await fireEvent.click(screen.getByRole('radio', { name: 'Only these agents' }))
  await waitFor(() => expect(saved().messaging).toEqual({ allow: [] }))
})

it('keeps the deprecated disabled tools under Advanced', async () => {
  const { invoke, saved } = machine()
  render(AgentProfile, { agent, tauri: { invoke }, catalog, onchange: vi.fn() })
  const list = screen.getByRole('list', { name: 'Disabled tools' })
  expect(within(list).getByText('powershell')).toBeInTheDocument()
  await fireEvent.input(screen.getByRole('combobox', { name: 'Tool id' }), { target: { value: 'bash' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Disable' }))
  await waitFor(() => expect(saved().tools).toEqual({ disabled: ['powershell', 'bash'] }))
  await fireEvent.click(screen.getByRole('button', { name: 'Turn powershell on again' }))
  await waitFor(() => expect(saved().tools).toEqual({ disabled: [] }))
})

it('runs the agent on its own machine, lists its runs and deletes it with its memory on request', async () => {
  const remote = { ...agent, runtimeId: 'box' }
  const { invoke, requests } = machine(remote)
  const onrun = vi.fn()
  const ondelete = vi.fn()
  const onopen = vi.fn()
  const runs = [{ id: 'r1', agent_id: 'scout', thread_id: 't1', status: 'completed', trigger: { kind: 'cron' }, silent: true, created_at: '2026-01-01T14:00:00Z', updated_at: '2026-01-01T14:00:30Z' }]
  render(AgentProfile, { agent: remote, tauri: { invoke }, runs, runtimeList: { runtimes: [{ id: 'local', local: true, kind: 'local', name: 'This computer' }, { id: 'box', kind: 'container', name: 'Build box' }] }, onrun, ondelete, onopen, onchange: vi.fn() })
  // A paired machine's agent keeps its folder and memory there.
  expect(screen.queryByRole('button', { name: 'Open folder' })).not.toBeInTheDocument()
  expect(screen.queryByText('Memory')).not.toBeInTheDocument()
  expect(screen.getByRole('region', { name: 'Primary machine' })).toHaveTextContent('Build box')
  const activity = screen.getByRole('region', { name: 'Activity' })
  expect(activity).toHaveTextContent('Completed')
  expect(activity).toHaveTextContent('Schedule')
  expect(activity).toHaveTextContent('30s')
  expect(activity).toHaveTextContent('silent')
  await fireEvent.click(within(activity).getByRole('button', { name: 'Open conversation' }))
  expect(onopen).toHaveBeenCalledWith('t1')

  await fireEvent.click(screen.getByRole('button', { name: 'Run now' }))
  await waitFor(() => expect(onrun).toHaveBeenCalledWith(expect.objectContaining({ id: 'run-new', runtimeId: 'box' })))
  expect(requests.at(-1)).toEqual({ runtime: 'box', operation: 'agent.run_now', body: { id: 'scout' } })

  await fireEvent.click(screen.getByRole('button', { name: 'Delete agent' }))
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Also delete its memory' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }))
  await waitFor(() => expect(ondelete).toHaveBeenCalled())
  expect(requests.at(-1)).toEqual({ runtime: 'box', operation: 'agent.delete', body: { id: 'scout', remove_memory: true } })
})

it('opens scoped memory only on request', async () => {
  const invoke = vi.fn(async (command) => command === 'agent_memory' ? [{ id: 'fact', title: 'Source preference', content: 'Use primary sources.', source: 'Chat' }] : null)
  render(AgentProfile, { agent, tauri: { invoke } })
  expect(invoke.mock.calls.some(([command]) => command === 'agent_memory')).toBe(false)
  await fireEvent.click(screen.getByText('Memory', { exact: true }))
  await screen.findByText('Source preference')
  expect(invoke).toHaveBeenCalledWith('agent_memory', expect.objectContaining({ id: 'scout', action: 'memory_facts' }))
})

it('accepts or dismisses routines core proposed and edits a schedule', async () => {
  const proposed = { id: 'extract-1', name: 'Morning digest', enabled: false, proposed: true, created_from: 'instructions', prompt: 'Digest.', trigger: { kind: 'cron', expr: '0 8 * * 1-5', tz: 'UTC' }, last_fired_at: null, last_result: null }
  const urgent = { ...proposed, id: 'import-2', name: 'Urgent mail' }
  const daily = { id: 'schedule', name: 'Briefing', enabled: true, created_from: 'user', prompt: '', trigger: { kind: 'cron', expr: '0 9 * * *', tz: 'UTC' }, last_fired_at: '2026-01-01T09:00:00Z', last_result: { run_id: 'r', status: 'completed', silent: false } }
  const withRoutines = { ...agent, routines: [proposed, urgent, daily] }
  const { invoke, requests } = machine(withRoutines)
  const onchange = vi.fn()
  render(AgentProfile, { agent: withRoutines, tauri: { invoke }, onchange })
  const routines = screen.getByRole('region', { name: 'Routines' })
  expect(within(routines).getByText('Every day at 09:00 · UTC')).toBeInTheDocument()
  const digest = await screen.findByRole('article', { name: 'Proposed routine Morning digest' })
  expect(digest).toHaveTextContent('Proposed from instructions')
  expect(screen.getByRole('article', { name: 'Proposed routine Urgent mail' })).toHaveTextContent('Proposed from the imported template')
  expect(within(routines).queryByRole('button', { name: 'Morning digest' })).not.toBeInTheDocument()
  await fireEvent.click(within(digest).getByRole('button', { name: 'Accept' }))
  await waitFor(() => expect(requests.filter((request) => request.operation !== 'agent.routine_proposals').at(-1)).toEqual({ runtime: 'local', operation: 'agent.routine_proposal_accept', body: { agent_id: 'scout', routine_id: 'extract-1' } }))
  await fireEvent.click(within(screen.getByRole('article', { name: 'Proposed routine Urgent mail' })).getByRole('button', { name: 'Dismiss' }))
  await waitFor(() => expect(requests.filter((request) => request.operation !== 'agent.routine_proposals').at(-1)).toEqual({ runtime: 'local', operation: 'agent.routine_proposal_dismiss', body: { agent_id: 'scout', routine_id: 'import-2' } }))

  await fireEvent.click(within(routines).getByRole('button', { name: 'Briefing' }))
  const editor = await screen.findByRole('dialog', { name: 'Edit routine' })
  await fireEvent.change(within(editor).getByRole('combobox', { name: 'Repeat' }), { target: { value: 'weekly' } })
  await fireEvent.change(within(editor).getByRole('combobox', { name: 'Day' }), { target: { value: '5' } })
  expect(within(editor).getByRole('textbox', { name: 'Expression' })).toHaveValue('0 9 * * 5')
  await fireEvent.click(within(editor).getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(requests.filter((request) => request.operation !== 'agent.routine_proposals').at(-1)).toEqual({ runtime: 'local', operation: 'routine.save', body: { agent_id: 'scout', routine: { ...daily, trigger: { kind: 'cron', expr: '0 9 * * 5', tz: 'UTC' } } } }))
  await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Edit routine' })).not.toBeInTheDocument())
  expect(onchange).toHaveBeenCalled()
})

it('adds a tool check with its condition', async () => {
  const { invoke, requests } = machine()
  render(AgentProfile, { agent, tauri: { invoke }, catalog, onchange: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: 'Add routine' }))
  const editor = await screen.findByRole('dialog', { name: 'New routine' })
  await fireEvent.input(within(editor).getByRole('textbox', { name: 'Name' }), { target: { value: 'New leads' } })
  await fireEvent.change(within(editor).getByRole('combobox', { name: 'Starts on' }), { target: { value: 'mcp' } })
  await fireEvent.input(within(editor).getByRole('combobox', { name: 'Server' }), { target: { value: 'calendar' } })
  expect(within(editor).getByText('Test this server in Tools to list its tools.')).toBeInTheDocument()
  await fireEvent.input(within(editor).getByRole('combobox', { name: 'Server' }), { target: { value: 'crm' } })
  expect([...editor.querySelectorAll('datalist[id$="-servers"] option, datalist[id$="-tools"] option')].map((option) => option.value)).toEqual(['crm', 'calendar', 'search_leads', 'get_lead'])
  await fireEvent.input(within(editor).getByRole('combobox', { name: 'Tool' }), { target: { value: 'search_leads' } })
  await fireEvent.input(within(editor).getByRole('textbox', { name: 'Arguments' }), { target: { value: '{"status":"new"}' } })
  const check = within(editor).getByRole('group', { name: 'Check 1' })
  await fireEvent.input(within(check).getByRole('textbox', { name: 'Result path' }), { target: { value: '/total' } })
  await fireEvent.change(within(check).getByRole('combobox', { name: 'Comparison' }), { target: { value: 'gt' } })
  await fireEvent.input(within(check).getByRole('textbox', { name: 'Value' }), { target: { value: '0' } })
  await fireEvent.click(within(editor).getByRole('button', { name: 'Save' }))
  const changed = () => requests.filter((request) => request.operation !== 'agent.routine_proposals')
  await waitFor(() => expect(changed().at(-1)?.operation).toBe('routine.save'))
  const routine = changed().at(-1).body.routine
  expect(routine).toMatchObject({ id: '', name: 'New leads', enabled: true, created_from: 'user' })
  expect(routine.trigger).toEqual({ kind: 'mcp', server: 'crm', tool: 'search_leads', args: { status: 'new' }, every: { expr: '*/15 * * * *', tz: expect.any(String) }, condition: { all: [{ path: '/total', op: 'gt', value: 0 }] } })
})

const swarm = {
  self: { local: 'core-laptop', box: 'core-box' },
  views: {},
  members: [
    { runtime_id: 'core-laptop', runtime_name: 'Laptop', form_factor: 'laptop', online: true },
    { runtime_id: 'core-box', runtime_name: 'Build box', form_factor: 'server', online: true },
    { runtime_id: 'core-studio', runtime_name: 'Studio', form_factor: 'desktop', online: false },
  ],
  agents: [], groups: [], approvals: [], absence: { paused: false },
}

it('moves the agent after a confirm, through the machine that holds it', async () => {
  const placed = { ...agent, runtimeId: 'local', primaryRuntime: 'core-laptop' }
  const { invoke, requests } = machine(placed)
  invoke.mockImplementation(async (command, args) => {
    requests.push(args)
    if (args.operation === 'agent.move') return { status: 'moved', agent: {} }
    return { proposals: [] }
  })
  const onchange = vi.fn()
  render(AgentProfile, { agent: placed, tauri: { invoke }, swarm, onchange })
  await fireEvent.click(within(screen.getByRole('region', { name: 'Primary machine' })).getByRole('button', { name: 'Move' }))
  const dialog = await screen.findByRole('dialog', { name: 'Move agent' })
  const move = within(dialog).getByRole('button', { name: 'Move agent' })
  expect(move).toBeDisabled()
  await fireEvent.change(within(dialog).getByRole('combobox', { name: 'Move to' }), { target: { value: 'core-box' } })
  await fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Move Scout to Build box' }))
  await fireEvent.click(move)
  await waitFor(() => expect(requests.at(-1)).toEqual({ runtime: 'local', operation: 'agent.move', body: { id: 'scout', to: 'core-box', force: false } }))
  expect(await screen.findByRole('status')).toHaveTextContent('Moved to Build box.')
  expect(onchange).toHaveBeenCalled()
})

it('forces a move off an offline primary only with the warning acknowledged, on the target', async () => {
  const stranded = { ...agent, runtimeId: 'local', primaryRuntime: 'core-studio' }
  const requests = []
  const invoke = vi.fn(async (_command, args) => { requests.push(args); return args.operation === 'agent.move' ? { status: 'forced' } : { proposals: [] } })
  render(AgentProfile, { agent: stranded, tauri: { invoke }, swarm, onchange: vi.fn() })
  await fireEvent.click(within(screen.getByRole('region', { name: 'Primary machine' })).getByRole('button', { name: 'Move' }))
  const dialog = await screen.findByRole('dialog', { name: 'Move agent' })
  await fireEvent.change(within(dialog).getByRole('combobox', { name: 'Move to' }), { target: { value: 'core-box' } })
  expect(within(dialog).getByRole('note')).toHaveTextContent('Studio does not answer.')
  expect(within(dialog).queryByRole('checkbox', { name: 'Move Scout to Build box' })).not.toBeInTheDocument()
  await fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Force the move' }))
  await fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Move Scout to Build box' }))
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Force move' }))
  await waitFor(() => expect(requests.at(-1)).toEqual({ runtime: 'box', operation: 'agent.move', body: { id: 'scout', to: 'core-box', force: true } }))
})

it('shows a synced copy\'s profile with its machine and move, and no settings', async () => {
  const replica = { id: 'editor', name: 'Release editor', label: 'Editor', replica: true, runtimeId: 'local', primaryRuntime: 'core-box', primaryMachine: { name: 'Build box', kind: 'desktop', formFactor: 'server', online: true }, routines: [] }
  render(AgentProfile, { agent: replica, tauri: { invoke: vi.fn(async () => ({})) }, swarm })
  expect(screen.getByRole('region', { name: 'Primary machine' })).toHaveTextContent('Build box')
  expect(screen.getByRole('region', { name: 'Primary machine' })).toHaveTextContent('Synced copy')
  expect(screen.getByText(/live on Build box/)).toBeInTheDocument()
  expect(screen.queryByRole('region', { name: 'Permissions' })).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Run now' })).not.toBeInTheDocument()
})

it('exports through core for Muniment and for Grok Bot', async () => {
  const { invoke, requests } = machine()
  render(AgentProfile, { agent, tauri: { invoke }, onchange: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: 'Export for Grok Bot' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('agent_export_template', { content: '# Export', fileName: 'Scout.grok_bot.md' }))
  expect(requests.at(-1)).toEqual({ runtime: 'local', operation: 'agent.export', body: { id: 'scout', format: 'grok_bot' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Export for Muniment' }))
  await waitFor(() => expect(requests.at(-1)).toEqual({ runtime: 'local', operation: 'agent.export', body: { id: 'scout', format: 'muniment' } }))
})

it('creates a webhook routine and shows its secret once, then rotates it', async () => {
  const hook = { hook_id: 'h1', preset: 'github', urls: ['https://192.0.2.20:7443/hooks/h1'], certificate_sha256: 'ab'.repeat(32), listening: false }
  const requests = []
  const invoke = vi.fn(async (_command, args) => {
    requests.push(args)
    if (args.operation === 'routine.webhook_create') return { routine: { id: 'r1', ...args.body.routine }, secret: 'first-secret', hook }
    if (args.operation === 'routine.webhook_rotate') return { routine: { id: 'r1' }, secret: 'second-secret', hook }
    return { proposals: [] }
  })
  const onchange = vi.fn()
  render(AgentProfile, { agent, tauri: { invoke }, onchange })
  await fireEvent.click(screen.getByRole('button', { name: 'Add routine' }))
  const editor = await screen.findByRole('dialog', { name: 'New routine' })
  await fireEvent.input(within(editor).getByRole('textbox', { name: 'Name' }), { target: { value: 'CI failed' } })
  await fireEvent.change(within(editor).getByRole('combobox', { name: 'Starts on' }), { target: { value: 'webhook' } })
  await fireEvent.change(within(editor).getByRole('combobox', { name: 'Sender' }), { target: { value: 'github' } })
  expect(within(editor).getByText('Save to get the address and its secret.')).toBeInTheDocument()
  await fireEvent.click(within(editor).getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(requests.find((request) => request.operation === 'routine.webhook_create')?.body).toEqual({ agent_id: 'scout', routine: { name: 'CI failed', enabled: true, prompt: '', trigger: { kind: 'webhook', preset: 'github' } } }))
  expect(await within(editor).findByText('first-secret')).toBeInTheDocument()
  expect(within(editor).getByText('https://192.0.2.20:7443/hooks/h1')).toBeInTheDocument()
  expect(within(editor).getByText(/does not accept connections/)).toBeInTheDocument()
  expect(onchange).toHaveBeenCalled()
  await fireEvent.click(within(editor).getByRole('button', { name: 'Rotate again' }))
  expect(await within(editor).findByText('second-secret')).toBeInTheDocument()
  expect(requests.at(-1)).toEqual({ runtime: 'local', operation: 'routine.webhook_rotate', body: { agent_id: 'scout', routine_id: 'r1' } })
  await fireEvent.click(within(editor).getByRole('button', { name: 'Done' }))
  await waitFor(() => expect(screen.queryByRole('dialog', { name: 'New routine' })).not.toBeInTheDocument())
})

it('loads an existing webhook\'s address and shows a tool check\'s last poll', async () => {
  const hook = { hook_id: 'h1', preset: 'forgejo', urls: ['https://192.0.2.20:7443/hooks/h1'], certificate_sha256: 'cd'.repeat(32), listening: true }
  const routines = [
    { id: 'build', name: 'Build finished', enabled: true, created_from: 'user', prompt: '', trigger: { kind: 'webhook', hook_id: 'h1', preset: 'forgejo' }, last_fired_at: null, last_result: null },
    { id: 'leads', name: 'New leads', enabled: true, created_from: 'user', prompt: '', trigger: { kind: 'mcp', server: 'crm', tool: 'search', args: {}, every: { expr: '*/30 * * * *', tz: 'UTC' }, condition: { all: [] } }, last_fired_at: null, last_result: null, last_poll: { at: new Date().toISOString(), fired: false, failures: 0 } },
  ]
  const withHook = { ...agent, routines }
  const invoke = vi.fn(async (_command, args) => args.operation === 'routine.webhook_info' ? { hook } : { proposals: [] })
  render(AgentProfile, { agent: withHook, tauri: { invoke }, onchange: vi.fn() })
  expect(screen.getByRole('group', { name: 'New leads' })).toHaveTextContent('condition did not hold')
  await fireEvent.click(screen.getByRole('button', { name: 'Build finished' }))
  const editor = await screen.findByRole('dialog', { name: 'Edit routine' })
  expect(await within(editor).findByText('https://192.0.2.20:7443/hooks/h1')).toBeInTheDocument()
  expect(within(editor).getByText('cd'.repeat(32))).toBeInTheDocument()
  expect(within(editor).queryByText(/does not accept connections/)).not.toBeInTheDocument()
  expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'routine.webhook_info', body: { agent_id: 'scout', routine_id: 'build' } })
})

it('lists failed runs, replays one against the agent as it is now and links the new run', async () => {
  const replay = { replay_id: 'rp1', agent_id: 'scout', trigger: { kind: 'webhook', routine_id: 'hook' }, error: 'The model could not complete this reply.', failed_at: '2026-01-01T14:05:00Z', redacted: true, replayed_by: [], prompt_preview: 'Summarize the build.', prompt_bytes: 20, definition_changed: true }
  const run = { id: 'run-r', agent_id: 'scout', thread_id: null, status: 'queued', trigger: { kind: 'webhook', source: 'replay' }, origin_run_id: 'rp1' }
  const { invoke, requests } = machine({ ...agent, routines: [] })
  const answer = invoke.getMockImplementation()
  invoke.mockImplementation(async (command, args) => {
    if (args?.operation === 'agent.replays') { requests.push(args); return { replays: [replay], agent_version: 'v2' } }
    if (args?.operation === 'agent.replay') { requests.push(args); return { run, replay: { ...replay, replayed_by: ['run-r'] } } }
    return answer(command, args)
  })
  const onrun = vi.fn()
  const onopenrun = vi.fn()
  const view = render(AgentProfile, { agent: { ...agent, routines: [{ id: 'hook', name: 'Build finished', enabled: true, trigger: { kind: 'webhook' } }] }, tauri: { invoke }, catalog, onchange: vi.fn(), onrun, onopenrun })
  const section = await screen.findByRole('region', { name: 'Failed runs' })
  expect(await within(section).findByText('Webhook · Build finished')).toBeInTheDocument()
  expect(section).toHaveTextContent('The agent changed since this failure. A replay uses it as it is now.')
  expect(section).toHaveTextContent('Secrets in the input were removed.')
  await fireEvent.click(within(section).getByRole('button', { name: 'Replay' }))
  await waitFor(() => expect(requests).toContainEqual({ runtime: 'local', operation: 'agent.replay', body: { id: 'scout', replay_id: 'rp1' } }))
  expect(onrun).toHaveBeenCalledWith({ ...run, runtimeId: 'local' })
  expect(await within(section).findByText('Replayed 1 time.')).toBeInTheDocument()
  expect(within(section).getByText('It links here once it starts.')).toBeInTheDocument()
  await view.rerender({ runs: [{ ...run, runtimeId: 'local', status: 'running', thread_id: 'thread-r' }] })
  await fireEvent.click(await within(section).findByRole('button', { name: 'Open run' }))
  expect(onopenrun).toHaveBeenCalledWith(expect.objectContaining({ id: 'run-r', thread_id: 'thread-r' }))
})

it('says when a machine has no failed runs and shows the agent\'s activity', async () => {
  const { invoke } = machine()
  render(AgentProfile, { agent, tauri: { invoke }, catalog, activity: { state: 'waiting', action: 'bash `git push`', since: null }, onchange: vi.fn(), onactivity: vi.fn() })
  expect(await screen.findByText('No failed runs to replay.')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Open activity' })).toBeInTheDocument()
  expect(screen.getByText('Waiting for you: bash `git push`')).toBeInTheDocument()
})

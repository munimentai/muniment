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

it('turns skills on and off, picks a core mode from the composer menu and limits messages to chosen agents', async () => {
  const { invoke, saved } = machine()
  const others = [{ id: 'editor', name: 'Editor', runtimeId: 'box' }, { id: 'triage', name: 'Triage', runtimeId: 'box' }]
  const view = render(AgentProfile, { agent, tauri: { invoke }, agents: [agent, ...others], catalog, onchange: vi.fn() })
  // The tool switches, the rule lists and the disabled tools stay out of the profile.
  expect(screen.queryByRole('switch', { name: 'Shell commands on' })).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Add rule' })).not.toBeInTheDocument()
  expect(screen.queryByText(/Disabled tools/)).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('checkbox', { name: /^Code review/ }))
  await waitFor(() => expect(saved().skills).toEqual(['review']))

  const permissions = screen.getByRole('region', { name: 'Permissions' })
  await fireEvent.click(await within(permissions).findByRole('button', { name: 'Permissions: Ask before changes' }))
  const menu = within(permissions).getByRole('menu', { name: 'Modes' })
  expect(within(menu).getAllByRole('menuitemradio').map((item) => item.getAttribute('aria-label'))).toEqual(['Ask before changes', 'Plan first', "Don't ask", 'Bypass permissions'])
  expect(within(menu).getByRole('menuitemradio', { name: 'Bypass permissions' })).toBeDisabled()
  expect(menu).not.toHaveTextContent('Shift+Tab')
  await fireEvent.click(within(menu).getByRole('menuitemradio', { name: "Don't ask" }))
  await waitFor(() => expect(saved().permissions.mode).toBe('dont_ask'))
  expect(within(permissions).queryByRole('menu')).not.toBeInTheDocument()

  const messaging = screen.getByRole('region', { name: 'Messaging' })
  expect(within(messaging).getByRole('radio', { name: 'Any' })).toHaveAttribute('aria-checked', 'true')
  await fireEvent.click(within(messaging).getByRole('radio', { name: 'Only these' }))
  await waitFor(() => expect(saved().messaging).toEqual({ allow: [] }))
  await view.rerender({ agent: { ...agent, messaging: { allow: ['editor'] } } })
  expect(within(messaging).getByText('Editor')).toBeInTheDocument()
  expect(within(messaging).queryByRole('checkbox')).not.toBeInTheDocument()
  await fireEvent.click(within(messaging).getByRole('button', { name: 'Choose agents' }))
  const list = within(messaging).getByRole('menu', { name: 'Agents' })
  expect(within(list).getByRole('menuitemcheckbox', { name: 'Editor' })).toHaveAttribute('aria-checked', 'true')
  await fireEvent.click(within(list).getByRole('menuitemcheckbox', { name: 'Triage' }))
  await waitFor(() => expect(saved().messaging).toEqual({ allow: ['editor', 'triage'] }))
  await fireEvent.click(within(messaging).getByRole('radio', { name: 'Any' }))
  await waitFor(() => expect(saved().messaging).toEqual({ allow: 'any' }))
})

it('saves the project on a pick and picks the model and thinking level from the composer popups', async () => {
  const { invoke, saved } = machine()
  const inventory = { providers: [{ id: 'ollama', name: 'Ollama', source: 'local', models: [{ id: 'qwen3:8b', thinking_levels: ['low', 'high'], thinking_default: 'low' }, { id: 'llama4' }] }] }
  const onvisibility = vi.fn()
  const view = render(AgentProfile, { agent, tauri: { invoke }, projects: [['p', 'Research'], ['q', 'Sales']], inventory, onvisibility, onchange: vi.fn() })
  const project = screen.getByRole('region', { name: 'Project' })
  expect(screen.queryByRole('button', { name: 'Edit project' })).not.toBeInTheDocument()
  await fireEvent.click(within(project).getByRole('button', { name: 'Project: Research' }))
  await fireEvent.click(within(within(project).getByRole('menu', { name: 'Project' })).getByRole('menuitemradio', { name: 'Sales' }))
  await waitFor(() => expect(saved().projectId).toBe('q'))

  const model = screen.getByRole('region', { name: 'Model' })
  expect(within(model).queryByRole('combobox')).not.toBeInTheDocument()
  await fireEvent.click(within(model).getByRole('button', { name: 'Model: Default model' }))
  const picker = within(model).getByRole('dialog', { name: 'Model' })
  await fireEvent.click(within(picker).getByRole('button', { name: 'Hide llama4' }))
  expect(onvisibility).toHaveBeenCalledWith(expect.objectContaining({ choice: 'llama4' }), false)
  await fireEvent.click(within(picker).getByRole('button', { name: 'qwen3:8b' }))
  await waitFor(() => expect(saved().model).toEqual({ provider: 'ollama', id: 'qwen3:8b' }))

  await view.rerender({ agent: { ...agent, model: { provider: 'ollama', id: 'qwen3:8b' } } })
  await fireEvent.click(within(model).getByRole('button', { name: 'Thinking level: Low' }))
  await fireEvent.click(within(within(model).getByRole('menu', { name: 'Thinking level' })).getByRole('menuitemradio', { name: 'High' }))
  await waitFor(() => expect(saved().model).toEqual({ provider: 'ollama', id: 'qwen3:8b', thinking: 'high' }))
  // The picker's foot puts the agent back on the default model.
  await fireEvent.click(within(model).getByRole('button', { name: 'Model: qwen3:8b' }))
  expect(within(model).getByText('qwen3:8b · this agent')).toBeInTheDocument()
  await fireEvent.click(within(model).getByRole('button', { name: 'Use the default model' }))
  await waitFor(() => expect(saved().model).toBeNull())
})

it('keeps an always allow rule an approval saved while the profile was open', async () => {
  const push = { request_kind: 'command', resource: { type: 'command', arguments: ['bash', 'git push'], working_directory: '/work' } }
  // The open profile shows no rule, and the agent's machine already holds one.
  const { invoke, saved } = machine({ ...agent, permissions: { ...agent.permissions, always_allow: [push] } })
  render(AgentProfile, { agent, tauri: { invoke }, catalog, onchange: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: 'Permissions: Ask before changes' }))
  await fireEvent.click(screen.getByRole('menuitemradio', { name: "Don't ask" }))
  await waitFor(() => expect(saved().permissions).toEqual({ mode: 'dont_ask', always_allow: [push], ask_first: [] }))
})


it('runs the agent on its own machine, opens a run from its history and deletes it with its memory on request', async () => {
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
  // One machine in reach shows no primary machine to pick.
  expect(screen.queryByRole('region', { name: 'Primary machine' })).not.toBeInTheDocument()
  const history = screen.getByRole('region', { name: 'History' })
  const row = within(history).getByRole('button', { name: /^Schedule, / })
  expect(row).toHaveTextContent(/at \d{1,2}:\d{2}/)
  expect(row).toHaveTextContent('No reply needed')
  expect(row.closest('li')).toHaveAttribute('data-light', 'yellow')
  await fireEvent.click(row)
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
  await fireEvent.click(within(screen.getByRole('region', { name: 'Memory' })).getByRole('button', { name: 'Open memory' }))
  const dialog = await screen.findByRole('dialog', { name: 'Agent memory' })
  await within(dialog).findByText('Source preference')
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
  // A schedule names no time zone, and its switch is the small one.
  expect(within(routines).getByText('Every day at 09:00')).toBeInTheDocument()
  expect(within(routines).queryByText(/UTC/)).not.toBeInTheDocument()
  expect(within(routines).getByRole('switch', { name: 'Briefing on' })).toHaveClass('small')
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

it('moves the agent to the machine picked in its profile after a confirm, through the machine that holds it', async () => {
  const placed = { ...agent, runtimeId: 'local', primaryRuntime: 'core-laptop' }
  const { invoke, requests } = machine(placed)
  invoke.mockImplementation(async (command, args) => {
    requests.push(args)
    if (args.operation === 'agent.move') return { status: 'moved', agent: {} }
    return { proposals: [] }
  })
  const onchange = vi.fn()
  render(AgentProfile, { agent: placed, tauri: { invoke }, swarm, onchange })
  const row = screen.getByRole('region', { name: 'Primary machine' })
  await fireEvent.click(within(row).getByRole('button', { name: 'Primary machine: This computer' }))
  const menu = within(row).getByRole('menu', { name: 'Primary machine' })
  expect(within(menu).getAllByRole('menuitemradio').map((item) => item.textContent.trim())).toEqual(['This computer', 'Build box', 'Studio Offline'])
  expect(within(menu).getByRole('menuitemradio', { name: 'This computer' })).toHaveAttribute('aria-checked', 'true')
  await fireEvent.click(within(menu).getByRole('menuitemradio', { name: 'Build box' }))
  const dialog = await screen.findByRole('dialog', { name: 'Move agent' })
  const move = within(dialog).getByRole('button', { name: 'Move agent' })
  expect(move).toBeDisabled()
  expect(within(dialog).getByRole('combobox', { name: 'Move to' })).toHaveValue('core-box')
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
  await fireEvent.click(screen.getByRole('button', { name: /^Primary machine:/ }))
  await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Build box' }))
  const dialog = await screen.findByRole('dialog', { name: 'Move agent' })
  expect(within(dialog).getByRole('combobox', { name: 'Move to' })).toHaveValue('core-box')
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

it('loads an existing webhook\'s address and shows a tool check\'s failed poll', async () => {
  const hook = { hook_id: 'h1', preset: 'forgejo', urls: ['https://192.0.2.20:7443/hooks/h1'], certificate_sha256: 'cd'.repeat(32), listening: true }
  const routines = [
    { id: 'build', name: 'Build finished', enabled: true, created_from: 'user', prompt: '', trigger: { kind: 'webhook', hook_id: 'h1', preset: 'forgejo' }, last_fired_at: null, last_result: null },
    { id: 'leads', name: 'New leads', enabled: true, created_from: 'user', prompt: '', trigger: { kind: 'mcp', server: 'crm', tool: 'search', args: {}, every: { expr: '*/30 * * * *', tz: 'UTC' }, condition: { all: [] } }, last_fired_at: null, last_result: null, last_poll: { at: new Date().toISOString(), fired: false, failures: 2, error: 'The server did not answer.' } },
  ]
  const withHook = { ...agent, routines }
  const invoke = vi.fn(async (_command, args) => args.operation === 'routine.webhook_info' ? { hook } : { proposals: [] })
  render(AgentProfile, { agent: withHook, tauri: { invoke }, onchange: vi.fn() })
  expect(screen.getByRole('group', { name: 'New leads' })).toHaveTextContent('crm search · every 30 minutes')
  expect(screen.getByRole('group', { name: 'New leads' })).toHaveTextContent('failed: The server did not answer. · 2 failures in a row')
  expect(screen.getByRole('group', { name: 'Build finished' })).toHaveTextContent('Webhook · Forgejo')
  await fireEvent.click(screen.getByRole('button', { name: 'Build finished' }))
  const editor = await screen.findByRole('dialog', { name: 'Edit routine' })
  expect(await within(editor).findByText('https://192.0.2.20:7443/hooks/h1')).toBeInTheDocument()
  expect(within(editor).getByText('cd'.repeat(32))).toBeInTheDocument()
  expect(within(editor).queryByText(/does not accept connections/)).not.toBeInTheDocument()
  expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'routine.webhook_info', body: { agent_id: 'scout', routine_id: 'build' } })
})

it('lists runs in one history with stoplight colors, retries a failed run core kept and opens each run', async () => {
  const replay = { replay_id: 'rp1', agent_id: 'scout', thread_id: 't-fail', trigger: { kind: 'webhook', routine_id: 'hook' }, error: 'The model could not complete this reply.', failed_at: '2026-01-01T14:05:00Z', redacted: true, replayed_by: [], prompt_preview: 'Summarize the build.', prompt_bytes: 20, definition_changed: true }
  const done = { id: 'r-done', agent_id: 'scout', thread_id: 't-done', status: 'completed', trigger: { kind: 'cron', routine_id: 'daily' }, created_at: new Date().toISOString(), updated_at: new Date().toISOString() }
  const run = { id: 'run-r', agent_id: 'scout', thread_id: null, status: 'queued', trigger: { kind: 'webhook', source: 'replay' }, origin_run_id: 'rp1', created_at: new Date().toISOString() }
  const { invoke, requests } = machine({ ...agent, routines: [] })
  const answer = invoke.getMockImplementation()
  invoke.mockImplementation(async (command, args) => {
    if (args?.operation === 'agent.replays') { requests.push(args); return { replays: [replay], agent_version: 'v2' } }
    if (args?.operation === 'agent.replay') { requests.push(args); return { run, replay: { ...replay, replayed_by: ['run-r'] } } }
    return answer(command, args)
  })
  const onrun = vi.fn()
  const onopen = vi.fn()
  const onopenrun = vi.fn()
  const routines = [{ id: 'hook', name: 'Build finished', enabled: true, trigger: { kind: 'webhook' } }, { id: 'daily', name: 'Morning digest', enabled: true, trigger: { kind: 'cron', expr: '0 9 * * *', tz: 'UTC' } }]
  const view = render(AgentProfile, { agent: { ...agent, routines }, tauri: { invoke }, runs: [done], catalog, onchange: vi.fn(), onrun, onopen, onopenrun })
  const history = screen.getByRole('region', { name: 'History' })
  // Failed runs core kept join the runs, newest first, with no separate list.
  const failed = await within(history).findByRole('button', { name: /^Build finished/ })
  expect(screen.queryByRole('region', { name: 'Failed runs' })).not.toBeInTheDocument()
  expect(within(history).getAllByRole('listitem').map((item) => item.dataset.light)).toEqual(['green', 'red'])
  expect(within(history).getByRole('button', { name: /^Morning digest, / })).toHaveTextContent(/^Morning digestToday at \d{1,2}:\d{2}/)
  expect(failed).toHaveTextContent(/Jan 1 at \d{1,2}:\d{2}.* · Failed$/)
  expect(history).not.toHaveTextContent(/UTC|GMT/)
  await fireEvent.click(failed)
  expect(onopen).toHaveBeenCalledWith('t-fail')

  await fireEvent.click(within(history).getByRole('button', { name: 'Retry Build finished' }))
  await waitFor(() => expect(requests).toContainEqual({ runtime: 'local', operation: 'agent.replay', body: { id: 'scout', replay_id: 'rp1' } }))
  expect(onrun).toHaveBeenCalledWith({ ...run, runtimeId: 'local' })
  // The retry opens through its run until it starts its conversation.
  await view.rerender({ runs: [{ ...run, runtimeId: 'local' }, done] })
  const retry = within(history).getByRole('button', { name: /^Retry, Today at/ })
  expect(retry.closest('li')).toHaveAttribute('data-light', 'yellow')
  expect(retry).toHaveTextContent('Queued')
  await fireEvent.click(retry)
  expect(onopenrun).toHaveBeenCalledWith(expect.objectContaining({ id: 'run-r' }))
  await view.rerender({ runs: [{ ...run, runtimeId: 'local', status: 'running', thread_id: 'thread-r' }, done] })
  await fireEvent.click(within(history).getByRole('button', { name: /^Retry, Today at/ }))
  expect(onopen).toHaveBeenCalledWith('thread-r')
})

it('says when the agent has no runs yet and shows its activity', async () => {
  const { invoke } = machine()
  render(AgentProfile, { agent, tauri: { invoke }, catalog, activity: { state: 'waiting', action: 'Run `git push` in work', since: null }, onchange: vi.fn() })
  expect(within(screen.getByRole('region', { name: 'History' })).getByText('No runs yet.')).toBeInTheDocument()
  expect(screen.getByText('Waiting for you: Run git push in work')).toBeInTheDocument()
})

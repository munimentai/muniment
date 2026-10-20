import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentCard from './AgentCard.svelte'
afterEach(cleanup)

const agent = {
  id: 'scout', name: 'Scout', label: 'Researcher', instructions: 'Find sources.', runtimeId: 'box', projectId: null, schedule: null,
  model: { provider: 'ollama', id: 'qwen3:8b', thinking: 'medium' },
  permissions: { mode: 'manual' },
  routines: [{ id: 'daily', name: 'Daily brief', enabled: true, prompt: '', trigger: { kind: 'cron', expr: '0 9 * * *', tz: 'UTC' } }],
}
const machine = { id: 'box', name: 'Build box', kind: 'container' }
const inventory = {
  providers: [
    { id: 'google', name: 'Google', source: 'key', models: [{ id: 'gemini-3-pro', context: '1M' }] },
    { id: 'ollama', name: 'Ollama', source: 'local', models: [{ id: 'qwen3:8b', context: '40K' }] },
  ],
  default_provider: 'ollama', default_model: 'qwen3:8b', hidden: [],
}

// Answers `agent_request` from the agent the machine stores and records each request.
function core({ refuse = null } = {}) {
  const stored = { ...structuredClone(agent), instructions: 'Edited on the machine.' }
  const requests = []
  const invoke = vi.fn(async (command, args) => {
    if (command !== 'agent_request') return null
    requests.push(args)
    if (args.operation === 'agent.read') return { agent: structuredClone(stored), primary_thread_id: null }
    if (args.operation === 'agent.save') {
      if (refuse) throw refuse
      return { agent: args.body.agent }
    }
    if (args.operation === 'agent.runs') return { runs: [], next_cursor: null }
    return {}
  })
  const saved = () => requests.filter((request) => request.operation === 'agent.save').at(-1)
  return { tauri: { invoke }, requests, saved, stored }
}

const show = (props) => render(AgentCard, { agent, machine, inventory, ...props })

it('renames the agent in place: Enter saves through agent.save, Escape cancels', async () => {
  const machineCore = core()
  const onchange = vi.fn()
  show({ tauri: machineCore.tauri, onchange })
  await fireEvent.click(screen.getByRole('button', { name: 'Edit agent name: Scout' }))
  const field = screen.getByRole('textbox', { name: 'Agent name' })
  await fireEvent.input(field, { target: { value: 'Not this' } })
  await fireEvent.keyDown(field, { key: 'Escape' })
  expect(screen.queryByRole('textbox', { name: 'Agent name' })).not.toBeInTheDocument()
  expect(machineCore.requests).toEqual([])

  await fireEvent.click(screen.getByRole('button', { name: 'Edit agent name: Scout' }))
  await fireEvent.input(screen.getByRole('textbox', { name: 'Agent name' }), { target: { value: '  Research scout ' } })
  await fireEvent.keyDown(screen.getByRole('textbox', { name: 'Agent name' }), { key: 'Enter' })
  await waitFor(() => expect(onchange).toHaveBeenCalled())
  expect(machineCore.requests[0]).toEqual({ runtime: 'box', operation: 'agent.read', body: { id: 'scout' } })
  // The save keeps what the machine holds and changes the name alone.
  expect(machineCore.saved()).toEqual({ runtime: 'box', operation: 'agent.save', body: { agent: { ...machineCore.stored, name: 'Research scout', schedule: null } } })
  await waitFor(() => expect(screen.queryByRole('textbox', { name: 'Agent name' })).not.toBeInTheDocument())
})

it('saves the name on blur and shows core\'s refusal, keeping the field to fix', async () => {
  const machineCore = core({ refuse: 'Keep the agent name under 100 bytes.' })
  show({ tauri: machineCore.tauri })
  await fireEvent.click(screen.getByRole('button', { name: 'Edit agent name: Scout' }))
  const field = screen.getByRole('textbox', { name: 'Agent name' })
  await fireEvent.input(field, { target: { value: 'A very long name' } })
  await fireEvent.blur(field)
  expect(await screen.findByRole('alert')).toHaveTextContent('Keep the agent name under 100 bytes.')
  expect(screen.getByRole('textbox', { name: 'Agent name' })).toHaveValue('A very long name')
  await fireEvent.keyDown(screen.getByRole('textbox', { name: 'Agent name' }), { key: 'Escape' })
  expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Edit agent name: Scout' })).toBeInTheDocument()
})

it('switches panels from the rail by click and by arrow keys, with no Tools panel', async () => {
  show({ tauri: core().tauri })
  const rail = screen.getByRole('tablist', { name: 'Scout panels' })
  const tab = (name) => within(rail).getByRole('tab', { name })
  expect(within(rail).getAllByRole('tab').map((entry) => entry.getAttribute('aria-label'))).toEqual(['Overview', 'Model', 'Permissions', 'Routines', 'History'])
  expect(tab('Overview')).toHaveAttribute('aria-selected', 'true')
  expect(tab('Overview')).toHaveAttribute('tabindex', '0')
  expect(tab('Model')).toHaveAttribute('tabindex', '-1')
  expect(screen.getByRole('tabpanel')).toHaveTextContent('Researcher')

  await fireEvent.click(tab('Routines'))
  expect(tab('Routines')).toHaveAttribute('aria-selected', 'true')
  expect(screen.getByRole('tabpanel')).toHaveTextContent('Daily brief')

  await fireEvent.keyDown(tab('Routines'), { key: 'ArrowDown' })
  expect(tab('History')).toHaveAttribute('aria-selected', 'true')
  await waitFor(() => expect(tab('History')).toHaveFocus())
  await fireEvent.keyDown(tab('History'), { key: 'ArrowDown' })
  expect(tab('Overview')).toHaveAttribute('aria-selected', 'true')
  await fireEvent.keyDown(tab('Overview'), { key: 'ArrowUp' })
  expect(tab('History')).toHaveAttribute('aria-selected', 'true')
  await fireEvent.keyDown(tab('History'), { key: 'Home' })
  expect(tab('Overview')).toHaveAttribute('aria-selected', 'true')
  await fireEvent.keyDown(tab('Overview'), { key: 'End' })
  expect(tab('History')).toHaveAttribute('aria-selected', 'true')
  expect(screen.getByRole('tabpanel')).toHaveAttribute('aria-labelledby', tab('History').id)
  expect(within(screen.getByRole('tabpanel')).getByRole('heading', { name: 'History' })).toBeInTheDocument()
  expect(await screen.findByText('No runs yet.')).toBeInTheDocument()
})

it('shows each state in words with its stoplight signal at the row end', () => {
  const view = show({ tauri: core().tauri, latestRun: { id: 'r1', status: 'completed', updated_at: new Date(Date.now() - 10 * 3600_000).toISOString() } })
  const fact = (name) => screen.getByText(name, { selector: 'dt' }).nextElementSibling
  const machineRow = fact('Machine')
  expect(machineRow).toHaveTextContent('Build box')
  expect(machineRow).not.toHaveTextContent('Online')
  const machineSignal = within(machineRow).getByRole('img', { name: 'Online' })
  expect(machineSignal).toHaveAttribute('data-light', 'green')
  expect(machineSignal).toHaveAttribute('data-tooltip', 'Online')
  expect(machineSignal.querySelector('svg')).toHaveAttribute('data-icon', 'container')
  expect(machineRow.lastElementChild).toBe(machineSignal)

  const lastRow = fact('Last run')
  expect(lastRow).toHaveTextContent('10h ago')
  expect(lastRow).not.toHaveTextContent('Completed')
  const runSignal = within(lastRow).getByRole('img', { name: 'Completed' })
  expect(runSignal).toHaveAttribute('data-light', 'green')
  expect(runSignal.querySelector('svg')).toHaveAttribute('data-icon', 'circle-check')
  expect(lastRow.lastElementChild).toBe(runSignal)

  view.rerender({ agent: { ...agent, online: false }, latestRun: { id: 'r2', status: 'failed', updated_at: new Date().toISOString() } })
  return Promise.resolve().then(() => {
    expect(within(fact('Machine')).getByRole('img', { name: 'Offline' })).toHaveAttribute('data-light', 'red')
    const failed = within(fact('Last run')).getByRole('img', { name: 'Failed' })
    expect(failed).toHaveAttribute('data-light', 'red')
    expect(failed.querySelector('svg')).toHaveAttribute('data-icon', 'circle-x')
  })
})

it('shows a running last run with the yellow signal', () => {
  show({ tauri: core().tauri, latestRun: { id: 'r1', status: 'running', updated_at: new Date().toISOString() } })
  const signal = within(screen.getByText('Last run', { selector: 'dt' }).nextElementSibling).getByRole('img', { name: 'Running' })
  expect(signal).toHaveAttribute('data-light', 'yellow')
})

it('picks the model from the composer picker open in the panel, or the default from its foot', async () => {
  const machineCore = core()
  show({ tauri: machineCore.tauri, onchange: vi.fn() })
  await fireEvent.click(screen.getByRole('tab', { name: 'Model' }))
  const picker = within(screen.getByRole('tabpanel')).getByRole('group', { name: 'Model' })
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  expect(within(picker).queryByRole('button', { name: 'Close model selector' })).not.toBeInTheDocument()
  expect(within(picker).getByRole('searchbox', { name: 'Search models' })).not.toHaveFocus()
  // The provider rail filters the list to the picked provider.
  expect(within(picker).getByRole('button', { name: 'qwen3:8b' })).toHaveAttribute('aria-pressed', 'true')
  expect(within(picker).queryByRole('button', { name: 'Gemini 3 Pro' })).not.toBeInTheDocument()
  await fireEvent.click(within(picker).getByRole('button', { name: 'Google' }))
  await fireEvent.click(within(picker).getByRole('button', { name: 'Gemini 3 Pro' }))
  await waitFor(() => expect(machineCore.saved()).toBeDefined())
  expect(machineCore.saved()).toEqual({ runtime: 'box', operation: 'agent.save', body: { agent: { ...machineCore.stored, model: { provider: 'google', id: 'gemini-3-pro', thinking: 'medium' }, schedule: null } } })

  await waitFor(() => expect(within(picker).getByRole('button', { name: 'Use the default model' })).not.toBeDisabled())
  await fireEvent.click(within(picker).getByRole('button', { name: 'Use the default model' }))
  await waitFor(() => expect(machineCore.saved().body.agent.model).toBeNull())
})

// Core's `permission.modes` answer on the agent's machine, with bypass off.
const modes = {
  modes: [
    { id: 'manual', title: 'Ask before changes', description: 'Reads run. Edits ask first.', available: true },
    { id: 'edit_auto', title: 'Edit automatically', description: 'Edits run.', available: true },
    { id: 'plan', title: 'Plan first', description: 'Plans first.', available: true },
    { id: 'auto', title: 'Auto review', description: 'A model reviews each action.', available: true },
    { id: 'dont_ask', title: "Don't ask", description: 'Denies the rest.', available: true },
    { id: 'bypass', title: 'Bypass permissions', description: 'Runs everything.', available: false, reason: 'Bypass is off.' },
  ],
  default_thread_mode: 'manual',
  default_agent_mode: 'auto',
}

it('picks a permission mode from the composer mode menu through agent.save, with no rules link', async () => {
  const machineCore = core()
  show({ tauri: machineCore.tauri, modes })
  await fireEvent.click(screen.getByRole('tab', { name: 'Permissions' }))
  const panel = screen.getByRole('tabpanel')
  expect(panel).not.toHaveTextContent('Rules')
  const button = within(panel).getByRole('button', { name: 'Permissions: Ask before changes' })
  await fireEvent.click(button)
  const menu = screen.getByRole('menu', { name: 'Modes' })
  expect(within(menu).getAllByRole('menuitemradio').map((item) => item.getAttribute('aria-label'))).toEqual(modes.modes.map((mode) => mode.title))
  expect(within(menu).getByRole('menuitemradio', { name: 'Ask before changes' })).toHaveAttribute('aria-checked', 'true')
  expect(within(menu).getByRole('menuitemradio', { name: 'Bypass permissions' })).toBeDisabled()
  expect(menu).not.toHaveTextContent('Shift+Tab')
  await fireEvent.click(within(menu).getByRole('menuitemradio', { name: "Don't ask" }))
  await waitFor(() => expect(machineCore.saved()).toBeDefined())
  expect(machineCore.saved().body.agent.permissions).toEqual({ ...agent.permissions, mode: 'dont_ask' })
  expect(screen.queryByRole('menu', { name: 'Modes' })).not.toBeInTheDocument()

  // Escape closes the menu without a change and returns to its button.
  await waitFor(() => expect(button).not.toBeDisabled())
  const saves = machineCore.requests.length
  await fireEvent.click(button)
  await fireEvent.keyDown(window, { key: 'Escape' })
  expect(screen.queryByRole('menu', { name: 'Modes' })).not.toBeInTheDocument()
  await waitFor(() => expect(button).toHaveFocus())
  expect(machineCore.requests).toHaveLength(saves)
})

it('marks the default mode of an agent that chose none, and names a refused change', async () => {
  const machineCore = core({ refuse: 'The agent settings are invalid.' })
  show({ tauri: machineCore.tauri, modes, agent: { ...agent, permissions: {} } })
  await fireEvent.click(screen.getByRole('tab', { name: 'Permissions' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Permissions: Auto review' }))
  const menu = screen.getByRole('menu', { name: 'Modes' })
  expect(menu).toHaveTextContent('Auto review · default')
  await fireEvent.click(within(menu).getByRole('menuitemradio', { name: 'Plan first' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('The agent settings are invalid.')
})

it('says so when the machine lists no modes', async () => {
  show({ tauri: core().tauri, modes: null })
  await fireEvent.click(screen.getByRole('tab', { name: 'Permissions' }))
  expect(screen.queryByRole('button', { name: /^Permissions:/ })).not.toBeInTheDocument()
  expect(screen.getByText('Update Build box to choose a mode.')).toBeInTheDocument()
})

it('lists routines as the profile does and turns one on and off through routine.save', async () => {
  const machineCore = core()
  const onprofile = vi.fn()
  show({ tauri: machineCore.tauri, onprofile })
  await fireEvent.click(screen.getByRole('tab', { name: 'Routines' }))
  const row = screen.getByRole('group', { name: 'Daily brief' })
  // A schedule reads in local time and names no time zone.
  expect(within(row).getByText('Every day at 09:00')).toBeInTheDocument()
  expect(row).not.toHaveTextContent('UTC')
  expect(row.querySelector('svg[data-icon="clock"]')).not.toBeNull()
  const toggle = within(row).getByRole('switch', { name: 'Daily brief on' })
  await fireEvent.click(toggle)
  await waitFor(() => expect(machineCore.requests.at(-1)).toEqual({ runtime: 'box', operation: 'routine.save', body: { agent_id: 'scout', routine: { ...agent.routines[0], enabled: false } } }))
  await fireEvent.click(within(row).getByRole('button', { name: 'Edit Daily brief in the profile' }))
  expect(onprofile).toHaveBeenCalledWith(agent)
})

it('lists runs in its History as the profile does and opens a run\'s conversation', async () => {
  const machineCore = core()
  const run = { id: 'r1', agent_id: 'scout', thread_id: 't1', status: 'failed', trigger: { kind: 'cron', routine_id: 'daily' }, created_at: new Date().toISOString(), updated_at: new Date().toISOString() }
  const invoke = machineCore.tauri.invoke
  machineCore.tauri.invoke = vi.fn(async (command, args) => command === 'agent_request' && args.operation === 'agent.runs' ? { runs: [run], next_cursor: null } : invoke(command, args))
  const onopenrun = vi.fn()
  show({ tauri: machineCore.tauri, onopenrun })
  await fireEvent.click(screen.getByRole('tab', { name: 'History' }))
  const history = await screen.findByRole('region', { name: 'History' })
  const row = await within(history).findByRole('button', { name: /^Daily brief, Today at / })
  expect(row.closest('li')).toHaveAttribute('data-light', 'red')
  expect(machineCore.tauri.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'box', operation: 'agent.runs', body: { agent_id: 'scout', limit: 20 } })
  await fireEvent.click(row)
  expect(onopenrun).toHaveBeenCalledWith(expect.objectContaining({ id: 'r1', thread_id: 't1', runtimeId: 'box' }))
})

it('shows an agent whose machine is offline read only, with a note', async () => {
  show({ tauri: core().tauri, modes, agent: { ...agent, online: false, replica: true } })
  expect(screen.getByRole('note')).toHaveTextContent('Read only while Build box is offline.')
  expect(screen.getByRole('button', { name: 'Edit agent name: Scout' })).toBeDisabled()
  expect(screen.getByRole('button', { name: 'Run now' })).toBeDisabled()
  expect(screen.getByRole('button', { name: 'Open chat' })).not.toBeDisabled()
  await fireEvent.click(screen.getByRole('tab', { name: 'Permissions' }))
  expect(screen.getByRole('button', { name: 'Permissions: Ask before changes' })).toBeDisabled()
  await fireEvent.click(screen.getByRole('tab', { name: 'Model' }))
  expect(screen.getByRole('button', { name: 'qwen3:8b' })).toBeDisabled()
})

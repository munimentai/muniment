import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentCard from './AgentCard.svelte'
afterEach(cleanup)

const agent = {
  id: 'scout', name: 'Scout', label: 'Researcher', instructions: 'Find sources.', runtimeId: 'box', projectId: null, schedule: null,
  model: { provider: 'ollama', id: 'qwen3:8b', thinking: 'medium' },
  permissions: { mode: 'ask', always_allow: [{ request_kind: 'tool', resource: { type: 'tool', name: 'read', input: {} } }], ask_first: [] },
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

it('switches panels from the rail by click and by arrow keys', async () => {
  show({ tauri: core().tauri })
  const rail = screen.getByRole('tablist', { name: 'Scout panels' })
  const tab = (name) => within(rail).getByRole('tab', { name })
  expect(tab('Overview')).toHaveAttribute('aria-selected', 'true')
  expect(tab('Overview')).toHaveAttribute('tabindex', '0')
  expect(tab('Model')).toHaveAttribute('tabindex', '-1')
  expect(screen.getByRole('tabpanel')).toHaveTextContent('Researcher')

  await fireEvent.click(tab('Routines'))
  expect(tab('Routines')).toHaveAttribute('aria-selected', 'true')
  expect(screen.getByRole('tabpanel')).toHaveTextContent('Daily brief')

  await fireEvent.keyDown(tab('Routines'), { key: 'ArrowDown' })
  expect(tab('Activity')).toHaveAttribute('aria-selected', 'true')
  await waitFor(() => expect(tab('Activity')).toHaveFocus())
  await fireEvent.keyDown(tab('Activity'), { key: 'ArrowDown' })
  expect(tab('Overview')).toHaveAttribute('aria-selected', 'true')
  await fireEvent.keyDown(tab('Overview'), { key: 'ArrowUp' })
  expect(tab('Activity')).toHaveAttribute('aria-selected', 'true')
  await fireEvent.keyDown(tab('Activity'), { key: 'Home' })
  expect(tab('Overview')).toHaveAttribute('aria-selected', 'true')
  await fireEvent.keyDown(tab('Overview'), { key: 'End' })
  expect(tab('Activity')).toHaveAttribute('aria-selected', 'true')
  expect(screen.getByRole('tabpanel')).toHaveAttribute('aria-labelledby', tab('Activity').id)
  expect(await screen.findByText('No runs yet. Run now starts one.')).toBeInTheDocument()
})

it('sets the model reference through agent.save, or clears it for the default', async () => {
  const machineCore = core()
  show({ tauri: machineCore.tauri, onchange: vi.fn() })
  await fireEvent.click(screen.getByRole('tab', { name: 'Model' }))
  const models = screen.getByRole('radiogroup', { name: 'Model' })
  expect(within(models).getByRole('radio', { name: 'Qwen 3:8B' })).toHaveAttribute('aria-checked', 'true')
  await fireEvent.click(within(models).getByRole('radio', { name: 'Gemini 3 Pro' }))
  await waitFor(() => expect(machineCore.saved()).toBeDefined())
  expect(machineCore.saved()).toEqual({ runtime: 'box', operation: 'agent.save', body: { agent: { ...machineCore.stored, model: { provider: 'google', id: 'gemini-3-pro', thinking: 'medium' }, schedule: null } } })

  await waitFor(() => expect(within(models).getByRole('radio', { name: 'Default model' })).not.toBeDisabled())
  await fireEvent.click(within(models).getByRole('radio', { name: 'Default model' }))
  await waitFor(() => expect(machineCore.saved().body.agent.model).toBeNull())
})

it('sets a permission mode through agent.save and keeps the rules', async () => {
  const machineCore = core()
  show({ tauri: machineCore.tauri })
  await fireEvent.click(screen.getByRole('tab', { name: 'Permissions' }))
  const modes = screen.getByRole('radiogroup', { name: 'Permission mode' })
  // The agent's `ask` reads as Manual.
  expect(within(modes).getByRole('radio', { name: 'Manual' })).toHaveAttribute('aria-checked', 'true')
  expect(within(modes).getAllByRole('radio').map((radio) => radio.getAttribute('aria-label'))).toEqual(['Manual', 'Edit automatically', 'Plan', 'Auto', "Don't ask", 'Bypass permissions'])
  const bypass = within(modes).getByRole('radio', { name: 'Bypass permissions' })
  expect(bypass).toBeDisabled()
  expect(bypass).toHaveAccessibleDescription('This machine has not turned it on.')

  await fireEvent.click(within(modes).getByRole('radio', { name: "Don't ask" }))
  await waitFor(() => expect(machineCore.saved()).toBeDefined())
  expect(machineCore.saved().body.agent.permissions).toEqual({ ...agent.permissions, mode: 'strict' })
  await waitFor(() => expect(within(modes).getByRole('radio', { name: 'Plan' })).not.toBeDisabled())
  await fireEvent.click(within(modes).getByRole('radio', { name: 'Plan' }))
  await waitFor(() => expect(machineCore.saved().body.agent.permissions.mode).toBe('plan'))
})

it('renders the modes core reports and names a refused change', async () => {
  const machineCore = core({ refuse: 'Plan mode needs a newer runtime.' })
  const modes = [{ id: 'manual', title: 'Manual', description: 'Asks first.', available: true }, { id: 'plan', title: 'Plan', description: 'Plans first.', available: true }]
  show({ tauri: machineCore.tauri, modes, agent: { ...agent, permissions: { ...agent.permissions, mode: 'manual' } } })
  await fireEvent.click(screen.getByRole('tab', { name: 'Permissions' }))
  expect(screen.getAllByRole('radio').map((radio) => radio.getAttribute('aria-label'))).toEqual(['Manual', 'Plan'])
  await fireEvent.click(screen.getByRole('radio', { name: 'Plan' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('Plan mode needs a newer runtime.')
})

it('turns a routine on and off through routine.save and opens the profile to edit it', async () => {
  const machineCore = core()
  const onprofile = vi.fn()
  show({ tauri: machineCore.tauri, onprofile })
  await fireEvent.click(screen.getByRole('tab', { name: 'Routines' }))
  expect(screen.getByText('Every day at 09:00 · UTC')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('switch', { name: 'Daily brief on' }))
  await waitFor(() => expect(machineCore.requests.at(-1)).toEqual({ runtime: 'box', operation: 'routine.save', body: { agent_id: 'scout', routine: { ...agent.routines[0], enabled: false } } }))
  await fireEvent.click(screen.getByRole('button', { name: 'Edit Daily brief in the profile' }))
  expect(onprofile).toHaveBeenCalledWith(agent)
})

it('shows an agent whose machine is offline read only, with a note', async () => {
  show({ tauri: core().tauri, agent: { ...agent, online: false, replica: true } })
  expect(screen.getByRole('note')).toHaveTextContent('Build box is offline. This card is read only until it answers.')
  expect(screen.getByRole('button', { name: 'Edit agent name: Scout' })).toBeDisabled()
  expect(screen.getByRole('button', { name: 'Run now' })).toBeDisabled()
  expect(screen.getByRole('button', { name: 'Open chat' })).not.toBeDisabled()
  await fireEvent.click(screen.getByRole('tab', { name: 'Permissions' }))
  expect(screen.getAllByRole('radio').every((radio) => radio.disabled)).toBe(true)
})

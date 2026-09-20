import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import MemorySection from './MemorySection.svelte'
import AgentManager from './AgentManager.svelte'

afterEach(cleanup)
it('loads and replaces a profile, then saves and deletes a sourced fact', async () => {
  let facts = []
  const invoke = vi.fn(async (cmd, args) => {
    if (cmd === 'memory_profile_read') return '# Profile\n\nCall me Sam.'
    if (cmd === 'memory_facts') return [...facts]
    if (cmd === 'memory_fact_save') { facts = [{ ...args.fact, id: 'fact-1' }]; return facts[0] }
    if (cmd === 'memory_fact_delete') facts = []
    return null
  })
  render(MemorySection, { tauri: { invoke } })
  await waitFor(() => expect(screen.getByLabelText('Other profile details')).toHaveValue('Call me Sam.'))
  await fireEvent.input(screen.getByLabelText('Preferred name'), { target: { value: 'Alex' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save profile' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_profile_save', { content: expect.stringContaining('## Preferred name\n\nAlex') }))
  await fireEvent.input(screen.getByLabelText('Title'), { target: { value: 'Reply style' } })
  await fireEvent.input(screen.getByLabelText('Fact'), { target: { value: 'Use concise replies.' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save memory' }))
  await screen.findByText('Use concise replies.')
  await fireEvent.click(screen.getByRole('button', { name: 'Delete', exact: true }))
  expect(invoke).not.toHaveBeenCalledWith('memory_fact_delete', expect.anything())
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_delete', { id: 'fact-1' }))
})

it('saves an agent with its project and weekly schedule before queuing a run', async () => {
  let agents = []
  const invoke = vi.fn(async (cmd, args) => {
    if (cmd === 'agent_list') return { agents: [...agents], state: { runs: {}, threads: {} } }
    if (cmd === 'agent_save') { const agent = { ...args.agent, id: 'agent-1' }; agents = [agent]; return agent }
    return null
  })
  render(AgentManager, { tauri: { invoke }, projects: [['project-1','Research']], onclose: vi.fn(), onstart: vi.fn(), onopen: vi.fn() })
  await screen.findByLabelText('Name')
  await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'Scout' } })
  await fireEvent.input(screen.getByLabelText('Description'), { target: { value: 'Read sources and prepare a brief.' } })
  await fireEvent.change(screen.getByLabelText('Project'), { target: { value: 'project-1' } })
  expect(screen.queryByRole('button', { name: 'Run now' })).not.toBeInTheDocument()
  expect(screen.queryByLabelText('Run on a schedule')).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Create agent' }))
  await screen.findByRole('button', { name: 'Save changes' })
  expect(invoke).not.toHaveBeenCalledWith('agent_run', expect.anything())
  await fireEvent.click(screen.getByLabelText('Run on a schedule'))
  await fireEvent.change(screen.getByLabelText('Repeat'), { target: { value: 'weekly' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Run now' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('agent_run', { id: 'agent-1' }))
  expect(invoke).toHaveBeenCalledWith('agent_save', { agent: expect.objectContaining({ projectId: 'project-1', schedule: expect.objectContaining({ cadence: 'weekly', enabled: true }) }) })
  expect(invoke.mock.calls.findIndex(([cmd]) => cmd === 'agent_save')).toBeLessThan(invoke.mock.calls.findIndex(([cmd]) => cmd === 'agent_run'))
})

it('shows the agent catalog and opens each existing agent chat', async () => {
  const invoke = vi.fn(async () => ({ agents: [{ id: 'scout', name: 'Scout', instructions: 'Find sources.', projectId: 'p', schedule: { enabled: true, cadence: 'weekly', weekday: 0, time: '09:00' } }], state: { runs: {}, threads: {} } }))
  const onselect = vi.fn()
  render(AgentManager, { tauri: { invoke }, projects: [['p', 'Research']], onclose: vi.fn(), onselect })
  const card = await screen.findByRole('button', { name: /Scout Research Monday/ })
  expect(screen.getByRole('button', { name: 'New agent' })).toBeInTheDocument()
  await fireEvent.click(card)
  expect(onselect).toHaveBeenCalledWith(expect.objectContaining({ id: 'scout' }))
  expect(screen.queryByLabelText('Name')).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'New agent' }))
  expect(screen.getByLabelText('Name')).toHaveValue('')
})

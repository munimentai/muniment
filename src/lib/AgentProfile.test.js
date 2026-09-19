import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentProfile from './AgentProfile.svelte'
afterEach(cleanup)
const agent = { id: 'scout', name: 'Scout', instructions: 'Find sources.', projectId: 'p', schedule: { enabled: true, cadence: 'weekly', weekday: 2, time: '09:00' } }
it('shows static fields, cancels edits, and preserves current disk fields when saving one field', async () => {
  const invoke = vi.fn(async cmd => cmd === 'agent_list' ? { agents: [{ ...agent, instructions: 'Manually edited on disk.' }] } : null)
  const onchange = vi.fn()
  render(AgentProfile, { agent, tauri: { invoke }, projects: [['p', 'Research']], onchange, onclose: vi.fn() })
  expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
  expect(screen.getByText('Research')).toBeInTheDocument()
  expect(screen.getByText('Wednesday · 09:00')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Edit name' }))
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Cancel this' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  expect(invoke).not.toHaveBeenCalled()
  await fireEvent.click(screen.getByRole('button', { name: 'Edit name' }))
  expect(screen.getByRole('textbox', { name: 'Name' })).toHaveValue('Scout')
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Research scout' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save', exact: true }))
  await waitFor(() => expect(onchange).toHaveBeenCalled())
  expect(invoke).toHaveBeenCalledWith('agent_save', { agent: { ...agent, name: 'Research scout', instructions: 'Manually edited on disk.' } })
})
it('retains a failed edit for retry and can pause a schedule without losing its time', async () => {
  let fail = true
  const invoke = vi.fn(async cmd => {
    if (cmd === 'agent_list') return { agents: [agent] }
    if (cmd === 'agent_save' && fail) throw new Error('Disk unavailable')
  })
  render(AgentProfile, { agent, tauri: { invoke }, onchange: vi.fn(), onclose: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: 'Edit schedule' }))
  await fireEvent.click(screen.getByLabelText('Run on a schedule'))
  await fireEvent.click(screen.getByRole('button', { name: 'Save', exact: true }))
  expect(await screen.findByRole('alert')).toHaveTextContent('Disk unavailable')
  expect(screen.getByLabelText('Run on a schedule')).not.toBeChecked()
  fail = false
  await fireEvent.click(screen.getByRole('button', { name: 'Save', exact: true }))
  await waitFor(() => expect(screen.queryByRole('form')).not.toBeInTheDocument())
  expect(invoke).toHaveBeenCalledWith('agent_save', { agent: { ...agent, schedule: { ...agent.schedule, enabled: false } } })
})
it('opens scoped memory only on request and shows routine history', async () => {
  const invoke = vi.fn(async (command, args) => command === 'agent_memory' ? [{ id: 'fact', title: 'Source preference', content: 'Use primary sources.', source: 'Chat' }] : null)
  const onopen = vi.fn()
  render(AgentProfile, { agent, tauri: { invoke }, history: [{ runId: 'run-one', threadId: 'chat-one', status: 'completed', lastRun: 1 }], onopen })
  expect(invoke).not.toHaveBeenCalled()
  await fireEvent.click(screen.getByText('Memory', { exact: true }))
  await screen.findByText('Source preference')
  expect(invoke).toHaveBeenCalledWith('agent_memory', expect.objectContaining({ id: 'scout', action: 'memory_facts' }))
  expect(invoke.mock.calls.some(([command]) => command === 'memory_profile_read')).toBe(false)
  await fireEvent.click(screen.getByText('Routine history (1)'))
  await fireEvent.click(screen.getByRole('button', { name: 'Open conversation' }))
  expect(onopen).toHaveBeenCalledWith('chat-one')
})

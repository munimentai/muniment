import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import MemorySection from './MemorySection.svelte'
import AgentManager from './AgentManager.svelte'

afterEach(cleanup)
HTMLDialogElement.prototype.showModal = function () { this.open = true }
HTMLDialogElement.prototype.close = function () { this.open = false }
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
  await fireEvent.click(screen.getByRole('button', { name: 'Add memory' }))
  await fireEvent.input(screen.getByLabelText('Title'), { target: { value: 'Reply style' } })
  await fireEvent.input(screen.getByLabelText('Fact'), { target: { value: 'Use concise replies.' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save memory' }))
  expect(await screen.findByRole('button', { name: 'Reply style' })).toHaveAttribute('aria-expanded', 'true')
  await fireEvent.click(screen.getByRole('button', { name: 'Delete', exact: true }))
  expect(invoke).not.toHaveBeenCalledWith('memory_fact_delete', expect.anything())
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_delete', { id: 'fact-1' }))
})

it('keeps Save profile off while the profile matches the saved one', async () => {
  const invoke = vi.fn(async (cmd) => cmd === 'memory_profile_read' ? '# Profile\n\n## Name\n\nSam\n' : cmd === 'memory_facts' ? [] : null)
  render(MemorySection, { tauri: { invoke }, part: 'profile' })
  const name = screen.getByLabelText('Name')
  await waitFor(() => expect(name).toHaveValue('Sam'))
  const save = screen.getByRole('button', { name: 'Save profile' })
  expect(save).toBeDisabled()
  await fireEvent.input(name, { target: { value: 'Alex' } })
  expect(save).toBeEnabled()
  await fireEvent.input(name, { target: { value: 'Sam' } })
  expect(save).toBeDisabled()
  await fireEvent.input(name, { target: { value: 'Alex' } })
  await fireEvent.click(save)
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_profile_save', { content: expect.stringContaining('## Name\n\nAlex') }))
  await waitFor(() => expect(save).toBeDisabled())
})

it('starts agent creation in chat without a manual form', async () => {
  const invoke=vi.fn(async()=>({agents:[],state:{runs:{},threads:{}}}))
  const oncreate=vi.fn()
  render(AgentManager,{tauri:{invoke},oncreate,onclose:vi.fn()})
  await fireEvent.click(screen.getByRole('button',{name:'New agent'}))
  expect(oncreate).toHaveBeenCalledOnce()
  expect(screen.queryByLabelText('Name')).not.toBeInTheDocument()
  expect(invoke).not.toHaveBeenCalledWith('agent_save',expect.anything())
})
it('opens an existing agent dedicated chat', async () => {
  const invoke=vi.fn(async()=>({agents:[{id:'scout',name:'Scout',instructions:'Find sources.'}],state:{runs:{},threads:{}}}))
  const onselect=vi.fn()
  render(AgentManager,{tauri:{invoke},onselect,oncreate:vi.fn(),onclose:vi.fn()})
  await fireEvent.click(within(await screen.findByRole('article',{name:'Scout'})).getByRole('button',{name:'Open chat'}))
  expect(onselect).toHaveBeenCalledWith(expect.objectContaining({id:'scout'}))
})
it('reads an unsaved agent draft card as a draft, never as a saved agent', async () => {
  const invoke=vi.fn(async()=>({agents:[],state:{runs:{},threads:{}}}))
  const plan={threadId:'c1',kind:'agent',goal:'Create a reusable agent for my work',output:'A saved agent with a clear task, instructions, and expected output',resultId:null,turns:[]}
  render(AgentManager,{tauri:{invoke},pending:[plan],onopen:vi.fn(),oncreate:vi.fn(),onclose:vi.fn()})
  expect(await screen.findByText('Draft agent. Describe its task to shape it.')).toBeInTheDocument()
  expect(screen.queryByText(/A saved agent/)).toBeNull()
})

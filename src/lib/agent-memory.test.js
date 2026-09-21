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
  await fireEvent.click(await screen.findByRole('button',{name:/^Scout/}))
  expect(onselect).toHaveBeenCalledWith(expect.objectContaining({id:'scout'}))
})

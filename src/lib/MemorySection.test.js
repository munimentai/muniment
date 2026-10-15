import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import MemorySection from './MemorySection.svelte'

afterEach(cleanup)
HTMLDialogElement.prototype.showModal = function () { this.open = true }
HTMLDialogElement.prototype.close = function () { this.open = false }

const TRUSTS = ['user', 'conversation', 'external']
const many = (count) => Array.from({ length: count }, (_, index) => ({
  id: `f${index + 1}`,
  title: `Fact ${index + 1}`,
  content: `Content of fact ${index + 1}.`,
  source: `thread:t${index + 1} run:r${index + 1}`,
  trust: TRUSTS[index % 3],
}))

function setup(facts, deleted = []) {
  let saved = facts
  const invoke = vi.fn(async (cmd, args) => {
    if (cmd === 'memory_profile_read') return ''
    if (cmd === 'memory_facts') return [...saved]
    if (cmd === 'memory_deleted_facts') return [...deleted]
    if (cmd === 'memory_fact_save') {
      const fact = { ...args.fact, id: args.fact.id || 'new-fact', trust: 'user' }
      saved = [...saved.filter(f => f.id !== fact.id), fact]
      return fact
    }
    if (cmd === 'memory_recall_settings') return null
    return null
  })
  render(MemorySection, { tauri: { invoke }, part: 'memory' })
  return invoke
}

const list = () => screen.getByRole('list', { name: 'Saved memories' })

it('shows one compact row per fact and opens its details in place', async () => {
  setup(many(3))
  const row = await screen.findByRole('button', { name: 'Fact 2 Saved from a chat' })
  expect(row).toHaveAttribute('aria-expanded', 'false')
  expect(screen.queryByText('thread:t2 run:r2')).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument()
  await fireEvent.click(row)
  expect(row).toHaveAttribute('aria-expanded', 'true')
  const detail = document.getElementById(row.getAttribute('aria-controls'))
  expect(within(detail).getByText('thread:t2 run:r2')).toBeInTheDocument()
  expect(within(detail).getByText('Saved from a chat')).toBeInTheDocument()
  expect(within(detail).getByRole('button', { name: 'Edit' })).toBeInTheDocument()
  expect(within(detail).getByRole('button', { name: 'Delete' })).toBeInTheDocument()
  await fireEvent.click(row)
  expect(row).toHaveAttribute('aria-expanded', 'false')
  expect(screen.queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument()
})

it('marks only external facts and explains them', async () => {
  setup(many(3))
  const external = await screen.findByRole('button', { name: 'Fact 3 Unconfirmed' })
  expect(within(list()).getAllByText('Unconfirmed')).toHaveLength(1)
  expect(screen.getByRole('button', { name: 'Fact 1 Added by you' })).toBeInTheDocument()
  await fireEvent.click(external)
  expect(screen.getByText('Saved after web, file or tool content. Chats reach it only by search. Edit it to confirm it.')).toBeInTheDocument()
})

it('filters to unconfirmed facts and combines the filter with search', async () => {
  setup(many(9))
  await screen.findByRole('button', { name: 'Fact 1 Added by you' })
  await fireEvent.click(screen.getByRole('radio', { name: 'Unconfirmed (3)' }))
  expect(within(list()).getAllByRole('button').map(b => b.querySelector('.fact-title').textContent)).toEqual(['Fact 3', 'Fact 6', 'Fact 9'])
  await fireEvent.input(screen.getByLabelText('Search memories'), { target: { value: 'fact 6' } })
  expect(within(list()).getAllByRole('button')).toHaveLength(1)
  await fireEvent.click(screen.getByRole('radio', { name: 'All' }))
  await fireEvent.input(screen.getByLabelText('Search memories'), { target: { value: '' } })
  expect(within(list()).getAllByRole('button')).toHaveLength(9)
})

it('renders 50 rows at a time and shows more on request', async () => {
  setup(many(120))
  await screen.findByRole('button', { name: 'Fact 1 Added by you' })
  expect(within(list()).getAllByRole('listitem')).toHaveLength(50)
  expect(screen.getByText('50 of 120')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Show 50 more' }))
  expect(within(list()).getAllByRole('listitem')).toHaveLength(100)
  await fireEvent.click(screen.getByRole('button', { name: 'Show 20 more' }))
  expect(within(list()).getAllByRole('listitem')).toHaveLength(120)
  expect(screen.queryByText(/ of 120/)).not.toBeInTheDocument()
})

it('edits a fact inside its row and saves it as a user fact', async () => {
  const invoke = setup(many(3))
  await fireEvent.click(await screen.findByRole('button', { name: 'Fact 3 Unconfirmed' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
  const form = screen.getByRole('region', { name: 'Edit memory' })
  expect(within(list()).getByRole('region', { name: 'Edit memory' })).toBe(form)
  expect(within(form).getByLabelText('Title')).toHaveValue('Fact 3')
  expect(screen.getAllByLabelText('Title')).toHaveLength(1)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  await fireEvent.input(within(form).getByLabelText('Fact'), { target: { value: 'Checked by hand.' } })
  await fireEvent.click(within(form).getByRole('button', { name: 'Save memory' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_save', { fact: expect.objectContaining({ id: 'f3', content: 'Checked by hand.' }) }))
  const row = await screen.findByRole('button', { name: 'Fact 3 Added by you' })
  expect(row).toHaveAttribute('aria-expanded', 'true')
  expect(within(document.getElementById(row.getAttribute('aria-controls'))).getByText('Added by you')).toBeInTheDocument()
})

it('lists deleted facts as compact rows with Restore in the details', async () => {
  const invoke = setup(many(2), [{ id: 'd1', title: 'Old fact', content: 'Gone.', source: 'Added in settings', trust: 'user' }])
  await screen.findByRole('button', { name: 'Fact 1 Added by you' })
  await fireEvent.click(screen.getByRole('button', { name: 'Deleted memories' }))
  const deleted = await screen.findByRole('list', { name: 'Deleted memories' })
  await fireEvent.click(within(deleted).getByRole('button', { name: 'Old fact Added by you' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Restore Old fact' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_restore', { id: 'd1' }))
})

it('adds a memory from a dialog and opens the new fact in the list', async () => {
  const invoke = setup(many(120))
  await screen.findByRole('button', { name: 'Fact 1 Added by you' })
  expect(screen.queryByLabelText('Title')).not.toBeInTheDocument()
  const add = screen.getByRole('button', { name: 'Add memory' })
  expect(add.closest('.group-title')).toHaveTextContent(/^Saved memories\s*120$/)
  add.focus()
  await fireEvent.click(add)
  const dialog = screen.getByRole('dialog', { name: 'Add memory' })
  expect(dialog).toHaveAttribute('open')
  await waitFor(() => expect(within(dialog).getByLabelText('Title')).toHaveFocus())
  await fireEvent.input(within(dialog).getByLabelText('Title'), { target: { value: 'Zebra rule' } })
  await fireEvent.input(within(dialog).getByLabelText('Fact'), { target: { value: 'Stripes first.' } })
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Save memory' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_save', { fact: expect.objectContaining({ id: '', title: 'Zebra rule', content: 'Stripes first.' }) }))
  await waitFor(() => expect(dialog).not.toHaveAttribute('open'))
  expect(add).toHaveFocus()
  const row = await screen.findByRole('button', { name: 'Zebra rule Added by you' })
  expect(row).toHaveAttribute('aria-expanded', 'true')
})

it('closes the add dialog with Escape and returns focus to the plus button', async () => {
  const invoke = setup(many(2))
  const add = await screen.findByRole('button', { name: 'Add memory' })
  add.focus()
  await fireEvent.click(add)
  const dialog = screen.getByRole('dialog', { name: 'Add memory' })
  await fireEvent(dialog, new Event('cancel', { cancelable: true }))
  expect(dialog).not.toHaveAttribute('open')
  expect(screen.queryByLabelText('Title')).not.toBeInTheDocument()
  expect(add).toHaveFocus()
  expect(invoke).not.toHaveBeenCalledWith('memory_fact_save', expect.anything())
})

it('adds agent memories from the same dialog', async () => {
  const invoke = vi.fn(async (cmd, args) => {
    if (args?.action === 'memory_facts') return []
    if (args?.action === 'memory_fact_save') return { ...args.fact, id: 'a1' }
    return null
  })
  render(MemorySection, { tauri: { invoke }, agentId: 'scout' })
  await fireEvent.click(await screen.findByRole('button', { name: 'Add memory' }))
  const dialog = screen.getByRole('dialog', { name: 'Add memory' })
  await fireEvent.input(within(dialog).getByLabelText('Title'), { target: { value: 'Goal' } })
  await fireEvent.input(within(dialog).getByLabelText('Fact'), { target: { value: 'Find sources.' } })
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Save memory' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('agent_memory', expect.objectContaining({ id: 'scout', action: 'memory_fact_save' })))
  await waitFor(() => expect(dialog).not.toHaveAttribute('open'))
})

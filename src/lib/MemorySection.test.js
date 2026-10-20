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
    if (cmd === 'memory_fact_delete') { deleted = [...deleted, ...saved.filter(f => f.id === args.id)]; saved = saved.filter(f => f.id !== args.id); return null }
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
  expect(screen.getByText('Saved after web, file or tool content. Chats reach it only by search. Confirm or edit it to trust it.')).toBeInTheDocument()
})

it('filters to unconfirmed facts and combines the filter with search', async () => {
  setup(many(9))
  await screen.findByRole('button', { name: 'Fact 1 Added by you' })
  await fireEvent.click(screen.getByRole('radio', { name: 'Unconfirmed (3)' }))
  expect([...list().querySelectorAll('.fact-title')].map(title => title.textContent)).toEqual(['Fact 3', 'Fact 6', 'Fact 9'])
  await fireEvent.input(screen.getByLabelText('Search memories'), { target: { value: 'fact 6' } })
  expect(within(list()).getAllByRole('listitem')).toHaveLength(1)
  await fireEvent.click(screen.getByRole('radio', { name: 'All' }))
  await fireEvent.input(screen.getByLabelText('Search memories'), { target: { value: '' } })
  expect(within(list()).getAllByRole('listitem')).toHaveLength(9)
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
  await fireEvent.click(await screen.findByRole('button', { name: 'Fact 2 Saved from a chat' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
  const form = screen.getByRole('region', { name: 'Edit memory' })
  expect(within(list()).getByRole('region', { name: 'Edit memory' })).toBe(form)
  expect(within(form).getByLabelText('Title')).toHaveValue('Fact 2')
  expect(screen.getAllByLabelText('Title')).toHaveLength(1)
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  await fireEvent.input(within(form).getByLabelText('Fact'), { target: { value: 'Checked by hand.' } })
  await fireEvent.click(within(form).getByRole('button', { name: 'Save memory' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_save', { fact: expect.objectContaining({ id: 'f2', content: 'Checked by hand.' }) }))
  const row = await screen.findByRole('button', { name: 'Fact 2 Added by you' })
  expect(row).toHaveAttribute('aria-expanded', 'true')
  expect(within(document.getElementById(row.getAttribute('aria-controls'))).getByText('Added by you')).toBeInTheDocument()
})

const quick = (title) => screen.getByRole('button', { name: `${title} Unconfirmed` }).closest('li')

it('puts edit, delete and confirm icons only on unconfirmed rows', async () => {
  setup(many(3))
  await screen.findByRole('button', { name: 'Fact 3 Unconfirmed' })
  const row = quick('Fact 3')
  expect(within(row).getAllByRole('button').map(b => b.getAttribute('aria-label'))).toEqual([null, 'Edit memory', 'Delete memory', 'Confirm memory'])
  expect(within(screen.getByRole('button', { name: 'Fact 1 Added by you' }).closest('li')).getAllByRole('button')).toHaveLength(1)
  const head = screen.getByRole('button', { name: 'Fact 3 Unconfirmed' })
  await fireEvent.click(head)
  const detail = document.getElementById(head.getAttribute('aria-controls'))
  expect(within(detail).getByText('Content of fact 3.')).toBeInTheDocument()
  expect(within(detail).queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument()
  expect(within(detail).queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument()
  expect(document.querySelectorAll('[title]')).toHaveLength(0)
})

it('confirms an unconfirmed fact as a user fact from its row', async () => {
  const invoke = setup(many(9))
  await screen.findByRole('button', { name: 'Fact 3 Unconfirmed' })
  expect(screen.getByRole('radio', { name: 'Unconfirmed (3)' })).toBeInTheDocument()
  const head = screen.getByRole('button', { name: 'Fact 3 Unconfirmed' })
  await fireEvent.click(within(quick('Fact 3')).getByRole('button', { name: 'Confirm memory' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_save', { fact: expect.objectContaining({ id: 'f3', title: 'Fact 3', content: 'Content of fact 3.' }) }))
  const row = await screen.findByRole('button', { name: 'Fact 3 Added by you' })
  expect(row).toHaveAttribute('aria-expanded', 'false')
  expect(head).toHaveAttribute('aria-expanded', 'false')
  expect(screen.getByRole('radio', { name: 'Unconfirmed (2)' })).toBeInTheDocument()
})

it('opens an unconfirmed fact straight into edit mode from its icon', async () => {
  setup(many(3))
  await screen.findByRole('button', { name: 'Fact 3 Unconfirmed' })
  await fireEvent.click(within(quick('Fact 3')).getByRole('button', { name: 'Edit memory' }))
  expect(screen.getByRole('button', { name: 'Fact 3 Unconfirmed' })).toHaveAttribute('aria-expanded', 'true')
  const form = within(list()).getByRole('region', { name: 'Edit memory' })
  await waitFor(() => expect(within(form).getByLabelText('Title')).toHaveFocus())
  expect(within(form).getByLabelText('Title')).toHaveValue('Fact 3')
})

it('deletes an unconfirmed fact from its icon after the confirm step', async () => {
  const invoke = setup(many(3))
  await screen.findByRole('button', { name: 'Fact 3 Unconfirmed' })
  await fireEvent.click(within(quick('Fact 3')).getByRole('button', { name: 'Delete memory' }))
  expect(screen.getByRole('button', { name: 'Fact 3 Unconfirmed' })).toHaveAttribute('aria-expanded', 'true')
  expect(invoke).not.toHaveBeenCalledWith('memory_fact_delete', expect.anything())
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_fact_delete', { id: 'f3' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Fact 3 Unconfirmed' })).not.toBeInTheDocument())
  await fireEvent.click(screen.getByRole('button', { name: 'Deleted memories' }))
  expect(await within(await screen.findByRole('list', { name: 'Deleted memories' })).findByRole('button', { name: 'Fact 3 Unconfirmed' })).toBeInTheDocument()
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
  // Settings names itself as a new fact's source, with no agent avatar.
  expect(within(dialog).getByLabelText('Source')).toHaveValue('Added in settings')
  expect(dialog.querySelector('.agent-avatar')).toBeNull()
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

const proposals = () => [
  { id: 'p1', op: 'add', fact_id: null, before: null, after: { title: 'Standup time', content: 'Joins standup at 9:30.' }, source: 'thread:t9 run:r9', created: '2026-10-14T12:00:00Z' },
  { id: 'p2', op: 'update', fact_id: 'f1', before: { id: 'f1', title: 'Fact 1', content: 'Content of fact 1.', source: 'Added in settings', trust: 'user' }, after: { title: 'Fact 1', content: 'Revised fact 1.' }, source: 'thread:t8 run:r8', created: '2026-10-14T11:00:00Z' },
  { id: 'p3', op: 'delete', fact_id: 'f2', before: { id: 'f2', title: 'Fact 2', content: 'Content of fact 2.', source: 'thread:t2 run:r2', trust: 'conversation' }, after: null, source: 'thread:t7 run:r7', created: '2026-10-14T10:00:00Z' },
]

function setupPending(facts, waiting, { stale = [] } = {}) {
  let saved = facts
  let pending = waiting
  const invoke = vi.fn(async (cmd, args) => {
    if (cmd === 'memory_profile_read') return ''
    if (cmd === 'memory_facts') return [...saved]
    if (cmd === 'memory_pending') return [...pending]
    if (cmd === 'memory_pending_apply') {
      const proposal = pending.find(p => p.id === args.id)
      pending = pending.filter(p => p.id !== args.id)
      if (stale.includes(args.id)) throw 'This memory changed after the suggestion. Review it again.'
      if (proposal.op === 'delete') { saved = saved.filter(f => f.id !== proposal.fact_id); return null }
      const fact = { ...(proposal.before || { id: 'added', source: proposal.source }), ...proposal.after, trust: 'conversation' }
      saved = proposal.op === 'add' ? [...saved, fact] : saved.map(f => f.id === fact.id ? fact : f)
      return fact
    }
    if (cmd === 'memory_pending_dismiss') { pending = pending.filter(p => p.id !== args.id); return null }
    return null
  })
  render(MemorySection, { tauri: { invoke }, part: 'memory' })
  return invoke
}

const review = async () => {
  await fireEvent.click(await screen.findByRole('button', { name: 'Review' }))
  return screen.getByRole('list', { name: 'Suggested changes' })
}
const open = async (name) => {
  const row = within(screen.getByRole('list', { name: 'Suggested changes' })).getByRole('button', { name })
  await fireEvent.click(row)
  return document.getElementById(row.getAttribute('aria-controls'))
}

it('shows a suggested changes notice only while changes wait', async () => {
  setupPending(many(3), proposals())
  expect(await screen.findByText('3 suggested changes')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Review' })).toHaveAttribute('aria-expanded', 'false')
  expect(screen.queryByRole('list', { name: 'Suggested changes' })).not.toBeInTheDocument()
  cleanup()
  setupPending(many(3), [])
  await screen.findByRole('button', { name: 'Fact 1 Added by you' })
  expect(screen.queryByText(/suggested change/)).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Review' })).not.toBeInTheDocument()
})

it('lists one compact row per suggested change and opens its details', async () => {
  setupPending(many(3), proposals())
  const rows = within(await review()).getAllByRole('button')
  expect(rows.map(row => [row.querySelector('.op').textContent, row.querySelector('.fact-title').textContent])).toEqual([['Add', 'Standup time'], ['Update', 'Fact 1'], ['Delete', 'Fact 2']])
  expect(screen.getByRole('button', { name: 'Hide' })).toHaveAttribute('aria-expanded', 'true')
  const update = await open('Update Fact 1')
  expect(within(update).getByText('Before')).toBeInTheDocument()
  expect(within(update).getByText('Content of fact 1.')).toBeInTheDocument()
  expect(within(update).getByText('After')).toBeInTheDocument()
  expect(within(update).getByText('Revised fact 1.')).toBeInTheDocument()
  expect(within(update).getByText('thread:t8 run:r8')).toBeInTheDocument()
  expect(within(update).getByRole('button', { name: 'Apply' })).toBeInTheDocument()
  expect(within(update).getByRole('button', { name: 'Dismiss' })).toBeInTheDocument()
  const removal = await open('Delete Fact 2')
  expect(within(removal).getByText('Removes')).toBeInTheDocument()
  expect(within(removal).getByText('Content of fact 2.')).toBeInTheDocument()
  expect(document.querySelectorAll('[title]')).toHaveLength(0)
})

it('applies an update and a delete in place and refreshes the waiting changes', async () => {
  const invoke = setupPending(many(3), proposals())
  await review()
  await fireEvent.click(within(await open('Update Fact 1')).getByRole('button', { name: 'Apply' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_pending_apply', { id: 'p2' }))
  expect(await screen.findByText('2 suggested changes')).toBeInTheDocument()
  expect(within(list()).getByText('Revised fact 1.')).toBeInTheDocument()
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'memory_pending')).toHaveLength(2)
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'memory_facts')).toHaveLength(1)
  await fireEvent.click(within(await open('Delete Fact 2')).getByRole('button', { name: 'Apply' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Fact 2 Saved from a chat' })).not.toBeInTheDocument())
  expect(await screen.findByText('1 suggested change')).toBeInTheDocument()
})

it('keeps a failed apply in place with its error until it is dismissed', async () => {
  const invoke = setupPending(many(3), proposals(), { stale: ['p2'] })
  await review()
  await fireEvent.click(within(await open('Update Fact 1')).getByRole('button', { name: 'Apply' }))
  const alert = await screen.findByRole('alert')
  expect(alert).toHaveTextContent('This memory changed after the suggestion. Review it again.')
  const rows = within(screen.getByRole('list', { name: 'Suggested changes' })).getAllByRole('button', { expanded: false })
  expect(rows.map(row => row.querySelector('.fact-title').textContent)).toEqual(['Standup time', 'Fact 2'])
  const detail = alert.closest('.fact-detail')
  expect(within(detail).queryByRole('button', { name: 'Apply' })).not.toBeInTheDocument()
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'memory_facts')).toHaveLength(2)
  await fireEvent.click(within(detail).getByRole('button', { name: 'Dismiss' }))
  expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  expect(screen.getByText('2 suggested changes')).toBeInTheDocument()
  expect(invoke).not.toHaveBeenCalledWith('memory_pending_dismiss', expect.anything())
})

it('dismisses a suggested change and hides the notice when none wait', async () => {
  const invoke = setupPending(many(3), proposals().slice(0, 1))
  await review()
  await fireEvent.click(within(await open('Add Standup time')).getByRole('button', { name: 'Dismiss' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('memory_pending_dismiss', { id: 'p1' }))
  await waitFor(() => expect(screen.queryByText(/suggested change/)).not.toBeInTheDocument())
  expect(screen.queryByRole('list', { name: 'Suggested changes' })).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Standup time Saved from a chat' })).not.toBeInTheDocument()
})

it('reviews an agent memory change through the agent command', async () => {
  let pending = proposals().slice(0, 1)
  const invoke = vi.fn(async (cmd, args) => {
    if (args?.action === 'memory_facts') return []
    if (args?.action === 'memory_pending') return [...pending]
    if (args?.action === 'memory_pending_apply') { pending = []; return { id: 'a1', ...proposals()[0].after, source: 'thread:t9 run:r9', trust: 'conversation' } }
    return null
  })
  render(MemorySection, { tauri: { invoke }, agentId: 'scout' })
  await review()
  await fireEvent.click(within(await open('Add Standup time')).getByRole('button', { name: 'Apply' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('agent_memory', expect.objectContaining({ id: 'scout', action: 'memory_pending_apply', factId: 'p1' })))
  expect(invoke).toHaveBeenCalledWith('agent_memory', expect.objectContaining({ id: 'scout', action: 'memory_pending' }))
  expect(await screen.findByRole('button', { name: 'Standup time Saved from a chat' })).toBeInTheDocument()
  await waitFor(() => expect(screen.queryByText(/suggested change/)).not.toBeInTheDocument())
})

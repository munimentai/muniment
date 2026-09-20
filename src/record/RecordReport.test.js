import '@testing-library/jest-dom/vitest'
import { cleanup, render, fireEvent, within } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import RecordReport from './RecordReport.svelte'
import DuplicateReview from './DuplicateReview.svelte'
import RecordRelate from './RecordRelate.svelte'

afterEach(cleanup)

it('opens only affected records and prepares the chosen survivor without committing a merge', async () => {
  const details = Object.fromEntries(['person-1', 'person-2'].map((id) => [id, {
    entity: { id, kind: 'person', title: 'Alex Smith', data: { email: `${id}@example.com` } },
    kind: { name: 'person', schema: { properties: { email: { type: 'string' } } } },
    identities: [{ kind: 'email', value: `${id}@example.com`, source: id === 'person-1' ? 'Contacts' : 'Invoices' }],
    events: [{ id: `event-${id}`, at: '2026-01-01T00:00:00Z', verb: 'created', actor_id: 'opaque-id', actor_label: 'Owner' }],
  }]))
  const invoke = vi.fn(async (command, args) => {
    if (command === 'record_report') return { report: { open: 1, findings: [{ id: 'duplicates', category: 'identity', magnitude: 1, claim: 'possible duplicate people', consequence: 'compare before merging', evidence: [], record_groups: [['person-1', 'person-2']] }] } }
    if (command === 'record_entity') return { entity: details[args.entity] }
    throw new Error(command)
  })
  const onmerge = vi.fn()
  const view = render(RecordReport, { company: { id: 'company-1', name: 'Company' }, tauri: { invoke }, onmerge })
  await fireEvent.click(await view.findByRole('button', { name: 'Review' }))
  const records = await view.findAllByRole('article', { name: /^Record [12]: Alex Smith$/ })
  expect(records).toHaveLength(2)
  expect(within(records[0]).getByLabelText('Differing fields')).toHaveTextContent('person-1@example.com')
  await fireEvent.click(within(records[0]).getByText('Sources and all fields'))
  await fireEvent.click(within(records[1]).getByText('Sources and all fields'))
  expect(within(records[0]).getByRole('region', { name: 'Identities' })).toHaveTextContent('Source: Contacts')
  expect(within(records[1]).getByRole('region', { name: 'History' })).toHaveTextContent('Owner')
  expect(view.getByRole('button', { name: 'Preview merge' })).toBeDisabled()
  await fireEvent.click(within(records[0]).getByRole('button', { name: 'Keep this record' }))
  expect(onmerge).not.toHaveBeenCalled()
  await fireEvent.click(view.getByRole('button', { name: 'Preview merge' }))
  expect(onmerge).toHaveBeenCalledWith(details['person-2'], details['person-1'].entity)
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(['record_report', 'record_entity', 'record_entity'])
})

it('requires a duplicate choice for larger groups and clears it when the survivor changes', async () => {
  const records = ['a', 'b', 'c', 'd'].map(id => ({ entity: { id, title: 'Same name', data: { name: 'Same name' } } }))
  const onmerge = vi.fn()
  const view = render(DuplicateReview, { records, onmerge })
  const keep = view.getAllByRole('button', { name: 'Keep this record' })
  await fireEvent.click(keep[0])
  expect(view.getByRole('button', { name: 'Preview merge' })).toBeDisabled()
  await fireEvent.change(view.getByLabelText('Record to merge'), { target: { value: 'c' } })
  await fireEvent.click(view.getByRole('button', { name: 'Preview merge' }))
  expect(onmerge).toHaveBeenCalledWith(records[2], records[0].entity)
  await fireEvent.click(keep[2])
  expect(view.getByRole('button', { name: 'Preview merge' })).toBeDisabled()
})

it('prepares the selected merge preview without committing it', async () => {
  const propose = vi.fn(async () => ({ proposal: { id: 'preview', diff: {}, warnings: ['Check the fields.'] } }))
  const commit = vi.fn()
  const view = render(RecordRelate, { mode: 'merge', initialTarget: { id: 'keep', kind: 'person', title: 'Keep' }, detail: { entity: { id: 'remove', kind: 'person', title: 'Duplicate' } }, propose, commit })
  await view.findByRole('group', { name: 'Proposed merge' })
  expect(propose).toHaveBeenCalledTimes(1)
  expect(propose).toHaveBeenCalledWith({ op: 'merge', loser: 'entity:remove', survivor: 'entity:keep' })
  expect(commit).not.toHaveBeenCalled()
  expect(view.getByText('Check the fields.')).toBeInTheDocument()
})

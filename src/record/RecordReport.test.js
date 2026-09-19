import '@testing-library/jest-dom/vitest'
import { cleanup, render, fireEvent, within } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import RecordReport from './RecordReport.svelte'

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
  const records = await view.findAllByRole('article', { name: 'Alex Smith' })
  expect(records).toHaveLength(2)
  expect(within(records[0]).getByRole('region', { name: 'Identities' })).toHaveTextContent('Source: Contacts')
  expect(within(records[1]).getByRole('region', { name: 'History' })).toHaveTextContent('Owner')
  await fireEvent.click(view.getAllByRole('button', { name: 'Merge Alex Smith into this record' })[0])
  expect(onmerge).toHaveBeenCalledWith(details['person-2'], details['person-1'].entity)
  expect(invoke.mock.calls.map(([command]) => command)).toEqual(['record_report', 'record_entity', 'record_entity'])
})

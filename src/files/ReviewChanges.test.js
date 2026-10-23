import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import ReviewChanges from './ReviewChanges.svelte'

afterEach(cleanup)

const line = (kind, text, oldLineNumber, newLineNumber) => ({ kind, text, oldLineNumber, newLineNumber, segments: [] })
const turn = (state) => ({
  run_id: 'r1', available: true, more: 0,
  files: [{ path: 'notes.md', status: 'modified', additions: 1, deletions: 1, binary: false, state, hunks_applied: [state === 'applied'] }],
  diff: { schemaVersion: 1, id: 'turn-r1', truncated: false, files: [{ oldPath: 'notes.md', newPath: 'notes.md', status: 'modified', binary: false, hunks: [{ oldStart: 1, oldCount: 1, newStart: 1, newCount: 1, header: '@@ -1,1 +1,1 @@', lines: [line('deletion', 'Draft', 1), line('addition', 'Final', undefined, 1)] }] }] },
})

it('lists each turn\'s files and reverts one change through core', async () => {
  const calls = []
  const host = { invoke: vi.fn(async (_command, { operation, body }) => {
    calls.push([operation, body])
    if (operation === 'run.changes') return body.run_id === 'r1' ? turn('applied') : { run_id: 'r2', available: false, unavailable: 'This turn has no checkpoint.', files: [] }
    if (operation === 'run.revert') return { outcome: { path: body.path, state: 'reverted' }, changes: turn('reverted') }
  }) }
  const view = render(ReviewChanges, { tauri: host, runtime: 'studio', threadId: 't', turns: [{ runId: 'r2', prompt: 'Older turn' }, { runId: 'r1', prompt: 'Draft the notes' }], initialRunId: 'r1' })
  await view.findByText('Final')
  expect(await view.findByText('1 earlier turn has no saved changes to show.')).toBeInTheDocument()
  expect(view.queryByText('Older turn')).toBeNull()
  expect(view.getByRole('button', { name: /notes\.md/ })).toHaveAttribute('aria-current', 'true')
  await fireEvent.click(view.getByRole('button', { name: 'Revert change' }))
  expect(await view.findByRole('status')).toHaveTextContent('Reverted a change in notes.md.')
  expect(calls).toContainEqual(['run.revert', { thread_id: 't', run_id: 'r1', path: 'notes.md', hunk: 0 }])
  expect(view.getByRole('button', { name: 'Revert file' })).toBeDisabled()
  expect(view.getAllByText('Reverted').length).toBeGreaterThan(0)
})

it('names a revert core refuses', async () => {
  const host = { invoke: vi.fn(async (_command, { operation }) => {
    if (operation === 'run.changes') return turn('applied')
    throw new Error('The file changed after this turn. Revert it by hand or rewind.')
  }) }
  const view = render(ReviewChanges, { tauri: host, runtime: 'local', threadId: 't', turns: [{ runId: 'r1', prompt: 'Draft' }], initialRunId: 'r1' })
  await fireEvent.click(await view.findByRole('button', { name: 'Revert file' }))
  await waitFor(() => expect(view.getByRole('alert')).toHaveTextContent('The file changed after this turn.'))
})

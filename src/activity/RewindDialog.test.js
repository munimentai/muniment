import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import RewindDialog from './RewindDialog.svelte'

afterEach(cleanup)

const preview = (extra = {}) => ({
  thread_id: 't', run_id: 'r', available: true, turns: 2, conversation: 'restore',
  files: { files_available: true, restore: ['notes.md'], remove: ['draft.md'], cannot_restore: [{ path: 'demo.mov', reason: 'too_large' }], not_covered: ['.git'], token: 'token-1' },
  irreversible: [{ kind: 'command', count: 1, labels: ['git push origin main'] }],
  ...extra,
})

function tauri(answers) {
  return { invoke: vi.fn(async (command, { operation, body }) => {
    expect(command).toBe('agent_request')
    const answer = answers[operation]
    if (answer instanceof Error) throw answer
    return typeof answer === 'function' ? answer(body) : answer
  }) }
}

it('shows what returns and what cannot be undone, with Cancel focused', async () => {
  const host = tauri({ 'thread.rewind_preview': preview() })
  const view = render(RewindDialog, { tauri: host, runtime: 'local', threadId: 't', runId: 'r', prompt: 'Push the notes.' })
  await view.findByText('Restores 1 file:')
  expect(view.getByRole('button', { name: 'Cancel' })).toHaveFocus()
  expect(view.getByText(/drops this turn and 1 later turn/)).toBeInTheDocument()
  expect(view.getByText('draft.md')).toBeInTheDocument()
  expect(view.getByText('too large to save')).toBeInTheDocument()
  expect(view.getByText('git push origin main')).toBeInTheDocument()
  expect(view.getByText(/1 command ran\. Their changes inside the folder are restored/)).toBeInTheDocument()
  expect(host.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'thread.rewind_preview', body: { thread_id: 't', run_id: 'r' } })
})

it('rewinds the conversation and files with the preview token, or the conversation alone', async () => {
  const done = vi.fn()
  const bodies = []
  const host = tauri({ 'thread.rewind_preview': preview(), 'thread.rewind': (body) => { bodies.push(body); return { status: 'rewound', turns: 2 } } })
  const view = render(RewindDialog, { tauri: host, runtime: 'local', threadId: 't', runId: 'r', ondone: done })
  await fireEvent.click(await view.findByRole('button', { name: 'Rewind conversation and files' }))
  await waitFor(() => expect(done).toHaveBeenCalledWith({ status: 'rewound', turns: 2 }))
  await fireEvent.click(view.getByRole('button', { name: 'Rewind conversation only' }))
  await waitFor(() => expect(bodies).toEqual([{ thread_id: 't', run_id: 'r', token: 'token-1' }, { thread_id: 't', run_id: 'r' }]))
})

it('holds the rewind while another reply uses the folder, and reads the preview again after a refusal', async () => {
  const blocked = render(RewindDialog, { tauri: tauri({ 'thread.rewind_preview': preview({ blocked: 'Another reply is using this folder. Rewind after it finishes.' }) }), runtime: 'local', threadId: 't', runId: 'r' })
  expect(await blocked.findByRole('alert')).toHaveTextContent('Another reply is using this folder.')
  expect(blocked.getByRole('button', { name: 'Rewind conversation and files' })).toBeDisabled()
  cleanup()
  let reads = 0
  const host = tauri({ 'thread.rewind_preview': () => { reads += 1; return preview() }, 'thread.rewind': new Error('The folder changed after the preview. Review the rewind again.') })
  const view = render(RewindDialog, { tauri: host, runtime: 'local', threadId: 't', runId: 'r', ondone: vi.fn() })
  await fireEvent.click(await view.findByRole('button', { name: 'Rewind conversation and files' }))
  expect(await view.findByRole('alert')).toHaveTextContent('The folder changed after the preview.')
  await waitFor(() => expect(reads).toBe(2))
})

it('offers the conversation alone when no file changed or files cannot return', async () => {
  const view = render(RewindDialog, { tauri: tauri({ 'thread.rewind_preview': preview({ files: { files_available: false, unavailable: 'The folder holds more than 20000 files, so its files were not saved.', not_covered: [] } }) }), runtime: 'local', threadId: 't', runId: 'r' })
  expect(await view.findByText(/more than 20000 files/)).toBeInTheDocument()
  expect(view.getByRole('button', { name: 'Rewind conversation' })).toBeEnabled()
  expect(view.queryByRole('button', { name: 'Rewind conversation and files' })).toBeNull()
})

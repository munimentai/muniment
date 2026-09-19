import '@testing-library/jest-dom/vitest'
import { cleanup, render, fireEvent, waitFor } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import FilePanel from './FilePanel.svelte'
import FileChanges from './FileChanges.svelte'
afterEach(cleanup)
it('shows the selected file, ignores a late read and exposes read errors', async () => {
  let resolve
  const tauri = { invoke: vi.fn((_, { path }) => path === 'a.js' ? new Promise((r) => { resolve = r }) : Promise.resolve('const latest = 2')) }
  const view = render(FilePanel, { file: { path: 'a.js' }, tauri })
  await view.rerender({ file: { path: 'b.js' }, tauri })
  await waitFor(() => expect(view.getByLabelText('File contents')).toHaveTextContent('const latest = 2'))
  resolve('stale')
  await Promise.resolve()
  expect(view.queryByText('stale')).toBeNull()
  tauri.invoke.mockRejectedValue(new Error('File removed'))
  await fireEvent.click(view.getByLabelText('Reload file'))
  expect(await view.findByRole('alert')).toHaveTextContent('File removed')
})
it('opens the changed file list with the keyboard-accessible chip', async () => {
  const onopen = vi.fn()
  const file = { path: 'src/a.js', name: 'a.js', additions: 3, deletions: 1 }
  const view = render(FileChanges, { files: [file], onopen })
  await fireEvent.click(view.getByRole('button', { name: /1 file changed/ }))
  await fireEvent.click(view.getByRole('button', { name: /a.js/ }))
  expect(onopen).toHaveBeenCalledWith(file)
  expect(view.getByRole('button', { name: /1 file changed/ })).toHaveAttribute('aria-expanded', 'false')
})

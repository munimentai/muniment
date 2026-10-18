import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import FileChanges from './FileChanges.svelte'

afterEach(cleanup)

const files = [{ path: 'src/a.js', name: 'a.js', additions: 2, deletions: 1 }]

it('opens Review changes from the chip and from its file list', async () => {
  const onreview = vi.fn()
  const view = render(FileChanges, { files, onopen: vi.fn(), onreview })
  await fireEvent.click(view.getByRole('button', { name: '1 file changed, 2 lines added, 1 line removed' }))
  expect(onreview).toHaveBeenCalledOnce()
  await fireEvent.pointerEnter(view.getByRole('group', { name: 'Changed files' }), { pointerType: 'mouse' })
  await fireEvent.click(view.getByRole('button', { name: 'Review changes' }))
  expect(onreview).toHaveBeenCalledTimes(2)
})

it('opens its file list on click when no review is offered', async () => {
  const view = render(FileChanges, { files, onopen: vi.fn() })
  await fireEvent.click(view.getByRole('button', { name: '1 file changed, 2 lines added, 1 line removed' }))
  expect(view.getByRole('button', { name: 'src/a.js' })).toBeInTheDocument()
})

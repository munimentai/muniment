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

it('shows only the nonzero side of a file count', () => {
  const view = render(FileChanges, { files: [{ path: 'src/b.js', name: 'b.js', incomplete: true }], onopen: vi.fn() })
  const chip = view.getByRole('button', { name: '1 file changed, 1 file added or edited' })
  expect(chip).toHaveTextContent('+1 file')
  expect(chip).not.toHaveTextContent('0 files')
})

it('sits one layer under the composer panels, so an open model picker covers the chip', async () => {
  const fs = await import('node:fs')
  const zIndex = (file, selector) => Number(fs.readFileSync(file, 'utf8').match(new RegExp(`${selector.replace(/[[\]]/g, '\\$&')} \\{[^}]*z-index: (\\d+)`))[1])
  expect(zIndex('src/App.svelte', '.composer-chips')).toBeLessThan(zIndex('src/styles/base.css', '[data-composer-panel]'))
})

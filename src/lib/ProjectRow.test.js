import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import ProjectRow from './ProjectRow.svelte'

afterEach(cleanup)

it.each(['Agents', 'Artifacts'])('keeps %s panel, creation, and expansion actions separate', async (name) => {
  const onactivate = vi.fn(), onnew = vi.fn(), ontoggle = vi.fn()
  const { rerender } = render(ProjectRow, { name, label: name, icon: 'bot', expanded: false, onactivate, onnew, ontoggle, newLabel: `New ${name}` })
  await fireEvent.click(screen.getByRole('button', { name, exact: true }))
  expect(onactivate).toHaveBeenCalledOnce()
  expect(ontoggle).not.toHaveBeenCalled()
  await fireEvent.click(screen.getByRole('button', { name: `New ${name}` }))
  expect(onnew).toHaveBeenCalledOnce()
  await fireEvent.click(screen.getByRole('button', { name: `Expand ${name}` }))
  expect(ontoggle).toHaveBeenCalledOnce()
  await rerender({ expanded: true })
  const collapse = screen.getByRole('button', { name: `Collapse ${name}` })
  expect(collapse.getAttribute('aria-expanded')).toBe('true')
  expect(collapse.querySelector('[data-icon="chevron-down"]')).not.toBeNull()
})

it('uses the project name and chevron to toggle the same list', async () => {
  const ontoggle = vi.fn()
  render(ProjectRow, { name: 'Research', expanded: false, ontoggle, onnew: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: 'Project Research' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Expand Research' }))
  expect(ontoggle).toHaveBeenCalledTimes(2)
})

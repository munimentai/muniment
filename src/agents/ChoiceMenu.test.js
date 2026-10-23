import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ChoiceMenu from './ChoiceMenu.svelte'
afterEach(cleanup)

it('checks the chosen row, focuses it, moves with the arrows and closes on Escape', async () => {
  const onchoose = vi.fn()
  const onclose = vi.fn()
  render(ChoiceMenu, { label: 'Project', value: 'q', options: [{ value: '', label: 'No project' }, { value: 'p', label: 'Research', icon: 'folder' }, { value: 'q', label: 'Sales', icon: 'folder' }], note: 'New work uses this project.', onchoose, onclose })
  const sales = screen.getByRole('menuitemradio', { name: 'Sales' })
  expect(sales).toHaveAttribute('aria-checked', 'true')
  await vi.waitFor(() => expect(sales).toHaveFocus())
  await fireEvent.keyDown(screen.getByRole('menu', { name: 'Project' }), { key: 'ArrowDown' })
  expect(screen.getByRole('menuitemradio', { name: 'No project' })).toHaveFocus()
  await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Research' }))
  expect(onchoose).toHaveBeenCalledWith('p', true)
  expect(screen.getByText('New work uses this project.')).toBeInTheDocument()
  await fireEvent.keyDown(window, { key: 'Escape' })
  expect(onclose).toHaveBeenCalled()
})

it('lists checkboxes for several choices and names an empty list', async () => {
  const onchoose = vi.fn()
  render(ChoiceMenu, { label: 'Agents', multiple: true, values: ['editor'], options: [{ value: 'editor', label: 'Editor', agent: { id: 'editor', name: 'Editor' } }, { value: 'triage', label: 'Triage', agent: { id: 'triage', name: 'Triage' } }], onchoose, onclose: vi.fn() })
  expect(screen.getByRole('menuitemcheckbox', { name: 'Editor' })).toHaveAttribute('aria-checked', 'true')
  await fireEvent.click(screen.getByRole('menuitemcheckbox', { name: 'Editor' }))
  expect(onchoose).toHaveBeenCalledWith('editor', false)
  cleanup()
  render(ChoiceMenu, { label: 'Agents', multiple: true, options: [], empty: 'No other agents.', onchoose, onclose: vi.fn() })
  expect(screen.getByText('No other agents.')).toBeInTheDocument()
})

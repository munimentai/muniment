import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelPicker from './ModelPicker.svelte'
import ProjectRow from './ProjectRow.svelte'

afterEach(cleanup)

it('closes the model selector with its X or Escape outside the search field', async () => {
  const onclose = vi.fn()
  render(ModelPicker, { inventory: { providers: [] }, onclose })
  await fireEvent.click(screen.getByRole('button', { name: 'Close model selector' }))
  expect(onclose).toHaveBeenCalledTimes(1)
  await fireEvent.keyDown(window, { key: 'Escape' })
  expect(onclose).toHaveBeenCalledTimes(2)
})

it('opens project actions during a reply while keeping new thread disabled', async () => {
  const onrename = vi.fn()
  render(ProjectRow, { name: 'General', expanded: true, disabled: true, onrename })
  await fireEvent.click(screen.getByRole('button', { name: 'Actions for project General' }))
  expect(screen.getByRole('menuitem', { name: 'New thread' })).toBeDisabled()
  await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
  expect(onrename).toHaveBeenCalledTimes(1)
})

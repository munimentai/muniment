import { cleanup, render, screen, fireEvent } from '@testing-library/svelte'
import { afterEach, it, expect, vi } from 'vitest'
import AttachmentMenu from './AttachmentMenu.svelte'

afterEach(cleanup)

it('offers a folder picker and dismisses after selection', async () => {
  const onfolder = vi.fn()
  render(AttachmentMenu, { onfolder, onfiles: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: 'Add files or folders' }))
  await fireEvent.click(screen.getByRole('menuitem', { name: 'Add folder' }))
  expect(onfolder).toHaveBeenCalledOnce()
  expect(screen.queryByRole('menu')).toBeNull()
})
it('dismisses on Escape and outside pointer', async () => {
  render(AttachmentMenu, { onfolder: vi.fn(), onfiles: vi.fn() })
  const trigger = screen.getByRole('button')
  await fireEvent.click(trigger)
  await fireEvent.keyDown(window, { key: 'Escape' })
  expect(screen.queryByRole('menu')).toBeNull()
  await fireEvent.click(trigger)
  await fireEvent.pointerDown(document.body)
  expect(screen.queryByRole('menu')).toBeNull()
})

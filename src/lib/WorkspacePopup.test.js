import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { listen } from '@tauri-apps/api/event'
import WorkspacePopup from './WorkspacePopup.svelte'
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ show: async () => {}, hide: async () => {}, setFocus: async () => {}, onFocusChanged: async () => () => {} }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}), emitTo: vi.fn().mockResolvedValue() }))
afterEach(() => { cleanup(); delete document.documentElement.dataset.pointer; vi.clearAllMocks() })

const opened = async (payload) => {
  render(WorkspacePopup)
  await waitFor(() => expect(listen.mock.calls.some(([name]) => name === 'action-menu-open')).toBe(true))
  await listen.mock.calls.findLast(([name]) => name === 'action-menu-open')[1]({ payload: { id: 'menu', label: 'Research actions', items: [{ id: 0, name: 'New thread' }, { id: 1, name: 'Settings' }, { id: 2, name: 'Rename' }], ...payload } })
}

it('marks the focus source the opener names, so a click shows no fill on the first item', async () => {
  await opened({ pointer: true })
  expect(screen.getByRole('menuitem', { name: 'New thread' })).toHaveFocus()
  expect(document.documentElement).toHaveAttribute('data-pointer')
  cleanup()
  await opened({ pointer: false })
  expect(document.documentElement).not.toHaveAttribute('data-pointer')
})

it('moves focus to the item under the pointer', async () => {
  await opened({ pointer: true })
  await fireEvent.pointerMove(screen.getByRole('menuitem', { name: 'Rename' }))
  expect(screen.getByRole('menuitem', { name: 'Rename' })).toHaveFocus()
})

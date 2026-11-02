import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { emitTo, listen } from '@tauri-apps/api/event'
import { trackFocusSource } from './focus-source.js'
import WorkspacePopup from './WorkspacePopup.svelte'
import popupSource from './WorkspacePopup.svelte?raw'
const setTitle = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ show: async () => {}, hide: async () => {}, setFocus: async () => {}, setTitle, onFocusChanged: async () => () => {} }) }))
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

it('names the native window for the menu it shows, not the workspace tools', async () => {
  await opened({ pointer: true })
  expect(setTitle).toHaveBeenCalledWith('Research actions')
  expect(document.title).toBe('Research actions')
  expect(screen.getByRole('menu', { name: 'Research actions' })).toBeInTheDocument()
  cleanup()
  render(WorkspacePopup)
  await waitFor(() => expect(listen.mock.calls.some(([name]) => name === 'workspace-menu-open')).toBe(true))
  await listen.mock.calls.findLast(([name]) => name === 'workspace-menu-open')[1]({ payload: { selected: null, label: 'Workspace tools', pointer: true } })
  expect(setTitle).toHaveBeenLastCalledWith('Workspace tools')
})

it('draws a destructive item, such as a thread\'s Delete, in the danger color', async () => {
  await opened({ pointer: true, items: [{ id: 0, name: 'Rename' }, { id: 1, name: 'Archive' }, { id: 2, name: 'Delete', danger: true }] })
  expect(screen.getByRole('menuitem', { name: 'Delete' })).toHaveAttribute('data-danger')
  expect(screen.getByRole('menuitem', { name: 'Archive' })).not.toHaveAttribute('data-danger')
  expect(popupSource).toMatch(/button\[data-danger\]\s*\{\s*color:\s*var\(--danger\)/)
})

it('names the keys when arrow keys and Enter choose the item after a click opened the menu', async () => {
  // main.js tracks the input in the popup window as in every window.
  const stop = trackFocusSource(document.documentElement)
  await opened({ pointer: true })
  // The first Down shows the first item, and the second moves to the next.
  await fireEvent.keyDown(screen.getByRole('menuitem', { name: 'New thread' }), { key: 'ArrowDown' })
  expect(screen.getByRole('menuitem', { name: 'New thread' })).toHaveFocus()
  await fireEvent.keyDown(screen.getByRole('menuitem', { name: 'New thread' }), { key: 'ArrowDown' })
  expect(screen.getByRole('menuitem', { name: 'Settings' })).toHaveFocus()
  await fireEvent.keyDown(screen.getByRole('menuitem', { name: 'Settings' }), { key: 'Enter' })
  await fireEvent.click(screen.getByRole('menuitem', { name: 'Settings' }), { detail: 0 })
  await waitFor(() => expect(emitTo).toHaveBeenCalledWith('main', 'action-menu-select', { id: 'menu', item: 1, via: 'keys' }))
  stop()
})

it('names the pointer when a click chooses the item after the keys opened the menu', async () => {
  const stop = trackFocusSource(document.documentElement)
  await opened({ pointer: false })
  await fireEvent.pointerDown(screen.getByRole('menuitem', { name: 'Rename' }))
  await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
  await waitFor(() => expect(emitTo).toHaveBeenCalledWith('main', 'action-menu-select', { id: 'menu', item: 2, via: 'pointer' }))
  stop()
})

it('reaches the last item on the first Up arrow in a menu a click opened, and moves on from the first after the keys opened it', async () => {
  await opened({ pointer: true })
  await fireEvent.keyDown(screen.getByRole('menuitem', { name: 'New thread' }), { key: 'ArrowUp' })
  expect(screen.getByRole('menuitem', { name: 'Rename' })).toHaveFocus()
  cleanup()
  await opened({ pointer: false })
  await fireEvent.keyDown(screen.getByRole('menuitem', { name: 'New thread' }), { key: 'ArrowDown' })
  expect(screen.getByRole('menuitem', { name: 'Settings' })).toHaveFocus()
})

it('moves on from the item the pointer reached', async () => {
  await opened({ pointer: true })
  await fireEvent.pointerMove(screen.getByRole('menuitem', { name: 'Settings' }))
  await fireEvent.keyDown(screen.getByRole('menuitem', { name: 'Settings' }), { key: 'ArrowDown' })
  expect(screen.getByRole('menuitem', { name: 'Rename' })).toHaveFocus()
})

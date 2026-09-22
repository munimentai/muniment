import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { Window } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import WorkspaceMenu from './WorkspaceMenu.svelte'
vi.mock('@tauri-apps/api/window', () => ({Window: {getByLabel: vi.fn()}, getCurrentWindow: () => ({innerPosition: async () => ({x:100,y:200}), scaleFactor: async () => 2, onMoved: async () => () => {}, onResized: async () => () => {}})}))
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn().mockResolvedValue(() => {})}))
afterEach(() => { cleanup(); delete window.__TAURI_INTERNALS__; vi.clearAllMocks() })
it('opens the styled popup without suspending the browser and selects a tool', async () => {
  window.__TAURI_INTERNALS__ = {}
  const popup = {setSize:vi.fn().mockResolvedValue(),setPosition:vi.fn().mockResolvedValue(),emit:vi.fn().mockResolvedValue(),hide:vi.fn().mockResolvedValue()}
  Window.getByLabel.mockResolvedValue(popup)
  const onselect=vi.fn(), onopenchange=vi.fn()
  render(WorkspaceMenu,{selected:'browser',onselect,onopenchange})
  await fireEvent.click(screen.getByRole('button',{name:'Workspace tools'}))
  await waitFor(() => expect(popup.emit).toHaveBeenCalledWith('workspace-menu-open',{selected:'browser'}))
  expect(onopenchange).not.toHaveBeenCalledWith(true)
  expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  const handler=listen.mock.calls.find(([name])=>name==='workspace-menu-select')[1]
  handler({payload:'files'})
  expect(onselect).toHaveBeenCalledWith('files')
})
it('keeps a keyboard accessible menu in browser previews',async()=>{
  render(WorkspaceMenu,{onselect:vi.fn()})
  await fireEvent.click(screen.getByRole('button',{name:'Workspace tools'}))
  expect(screen.getByRole('menuitem',{name:'Browser'})).toHaveFocus()
  await fireEvent.keyDown(screen.getByRole('menu'),{key:'Escape'})
  expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  expect(screen.getByRole('button',{name:'Workspace tools'})).toHaveFocus()
})

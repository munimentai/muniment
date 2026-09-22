import { cleanup, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import WorkspacePanel from './WorkspacePanel.svelte'
vi.mock('@xterm/xterm', () => ({Terminal: class {
  options = {}; cols = 80; rows = 24
  loadAddon() {} open() {} reset() {} focus() {} write() {} dispose() {}
  onData() { return {dispose() {}} }
}}))
vi.mock('@xterm/addon-fit', () => ({FitAddon: class {fit() {}}}))
afterEach(cleanup)
it('keeps each context shell alive and restores it without restarting', async () => {
  vi.stubGlobal('ResizeObserver', class {observe() {} disconnect() {}})
  vi.stubGlobal('matchMedia', () => ({addEventListener() {}, removeEventListener() {}}))
  const invoke = vi.fn(async (command, args) => {
    if (command === 'workspace_folders') return [{path: `/work/${args.projectId || args.agentId}`}]
    if (command === 'terminal_start') return args.path
    if (command === 'terminal_read') return {bytes: [], exited: false}
  })
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'terminal', context: {projectId: 'project'}, onselect: vi.fn()}})
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('terminal_start', expect.objectContaining({path: '/work/project'})))
  await view.rerender({props: {context: {agentId: 'agent'}}})
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('terminal_start', expect.objectContaining({path: '/work/agent'})))
  expect(screen.getByRole('tab', {name: 'agent'})).toHaveAttribute('aria-selected', 'true')
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'terminal_close')).toHaveLength(0)
  await view.rerender({props: {context: {projectId: 'project'}}})
  await waitFor(() => expect(view.container.querySelector('.terminal-workspace:not(.hidden)')).toHaveTextContent('/work/project'))
  expect(screen.getByRole('tab', {name: 'project'})).toHaveAttribute('aria-selected', 'true')
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'terminal_start')).toHaveLength(2)
  view.unmount()
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'terminal_close')).toHaveLength(2)
  vi.unstubAllGlobals()
})

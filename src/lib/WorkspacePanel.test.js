import { cleanup, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import WorkspacePanel from './WorkspacePanel.svelte'
vi.mock('@xterm/xterm', () => ({Terminal: class {
  options = {}; cols = 80; rows = 24
  loadAddon() {} open() {} reset() {} focus() {} write() {} dispose() {} clear() {} attachCustomKeyEventHandler() {}
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
  // The panel imports the terminal lazily; loading it first keeps the start wait off the module transform time.
  await import('./TerminalWorkspace.svelte')
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'terminal', context: {projectId: 'project'}, onselect: vi.fn()}})
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('terminal_start', expect.objectContaining({path: '/work/project'})), {timeout: 10000})
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

it('keeps the start of an artifact name and ends a long one in an ellipsis with the full name as its tooltip', async () => {
  vi.stubGlobal('ResizeObserver', class {observe() {} disconnect() {}})
  const invoke = vi.fn(async (command) => {
    if (command === 'artifact_list') return [{id: 'a1', name: 'New Year Countdown', threadId: 't1'}]
    return null
  })
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'artifacts', requestedArtifact: 'a1', context: {threadId: 't1'}, threadId: 't1', onselect: vi.fn()}})
  const tab = await screen.findByRole('tab', {name: 'New Year Countdown'})
  const label = tab.querySelector('.tab-label')
  expect(label).toHaveTextContent('New Year Countdown')
  expect(label.classList.contains('fade-viewport')).toBe(false)
  expect(tab).toHaveAttribute('title', 'New Year Countdown')
  view.unmount()
  vi.unstubAllGlobals()
})

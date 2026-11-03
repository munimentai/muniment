import { cleanup, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import WorkspacePanel from './WorkspacePanel.svelte'
vi.mock('@xterm/xterm', () => ({Terminal: class {
  options = {}; cols = 80; rows = 24
  loadAddon() {} open() {} reset() {} focus() {} write() {} dispose() {} clear() {} attachCustomKeyEventHandler() {}
  onData() { return {dispose() {}} }
  onWriteParsed() { return {dispose() {}} }
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

it('keeps the start of a terminal tab name, so a thread folder never loses its first letters', async () => {
  vi.stubGlobal('ResizeObserver', class {observe() {} disconnect() {}})
  vi.stubGlobal('matchMedia', () => ({addEventListener() {}, removeEventListener() {}}))
  const invoke = vi.fn(async (command, args) => {
    if (command === 'workspace_folders') return [{path: '/work/Pong_test_renamed-0a1b2c3d'}]
    if (command === 'terminal_start') return args.path
    if (command === 'terminal_read') return {bytes: [], exited: false}
  })
  await import('./TerminalWorkspace.svelte')
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'terminal', context: {threadId: 't1'}, onselect: vi.fn()}})
  const tab = await screen.findByRole('tab', {name: 'Pong test renamed'}, {timeout: 10000})
  const label = tab.querySelector('.tab-label')
  expect(label).toHaveTextContent('Pong test renamed')
  expect(label.classList.contains('fade-viewport')).toBe(false)
  view.unmount()
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
it('names the Files tab after the open thread\'s folder, never the folder of a thread it passed while that folder still loaded', async () => {
  const answers = {}
  const invoke = vi.fn((command, args) => {
    if (command === 'workspace_folders') return new Promise(resolve => { answers[args.threadId] = () => resolve([{label: 'Thread workspace', path: `/home/sessions/${args.threadId === 't1' ? 'New_thread-c3fa5c5a' : 'New_agent-0b3e9d21'}`}]) })
    if (command === 'workspace_list') return Promise.resolve({path: args.path, entries: []})
  })
  const onfolder = vi.fn()
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'files', context: {threadId: 't1', threadTitle: 'New thread'}, threadId: 't1', onselect: vi.fn(), onfolder}})
  await waitFor(() => expect(answers.t1).toBeTypeOf('function'))
  // An agent creation chat opens before the earlier thread's folder answers.
  await view.rerender({props: {context: {threadId: 't2', threadTitle: 'New agent'}, threadId: 't2'}})
  await waitFor(() => expect(answers.t2).toBeTypeOf('function'))
  answers.t2()
  await waitFor(() => expect(screen.getByRole('tab', {name: 'New agent'})).toHaveAttribute('title', '/home/sessions/New_agent-0b3e9d21'))
  answers.t1()
  await new Promise(resolve => setTimeout(resolve, 20))
  expect(screen.getByRole('tab', {name: 'New agent'})).toHaveAttribute('title', '/home/sessions/New_agent-0b3e9d21')
  expect(screen.queryByRole('tab', {name: 'New thread'})).toBeNull()
  expect(onfolder).not.toHaveBeenCalledWith('/home/sessions/New_thread-c3fa5c5a')
})
it('names the Files tab after a creation chat\'s title, not its folder', async () => {
  const invoke = vi.fn(async (command, args) => command === 'workspace_folders' ? [{label: 'Thread workspace', path: '/home/sessions/New_thread-c3fa5c5a'}] : {path: args.path, entries: []})
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'files', context: {threadId: 't1', label: 'New agent'}, threadId: 't1', onselect: vi.fn()}})
  await waitFor(() => expect(screen.getByRole('tab', {name: 'New agent'})).toHaveAttribute('title', '/home/sessions/New_thread-c3fa5c5a'))
  // The chat's title changes with the agent's name, and the Files view stays.
  await view.rerender({props: {context: {threadId: 't1', label: 'Scout'}}})
  expect(screen.getByRole('tab', {name: 'Scout'})).toBeInTheDocument()
  expect(invoke.mock.calls.filter(([command]) => command === 'workspace_folders')).toHaveLength(1)
})
// A file tab belongs to the view that opened it. A deleted draft's tab
// closes, another thread's tab stays, and a Files tab whose hidden view has
// not read its folder takes the thread's title.
it('closes a deleted draft\'s file tabs and names a hidden Files tab after the thread', async () => {
  vi.stubGlobal('ResizeObserver', class {observe() {} disconnect() {}})
  vi.stubGlobal('matchMedia', () => ({matches: false, addEventListener() {}, removeEventListener() {}}))
  const invoke = vi.fn(async (command, args) => command === 'workspace_folders' ? [{label: 'Thread workspace', path: `/home/sessions/${args.threadId}`}] : command === 'workspace_list' ? {path: args.path, entries: []} : '')
  const onselect = vi.fn()
  const draftFile = {path: '/home/sessions/New_agent-0b3e9d21/live37-draft.txt', name: 'live37-draft.txt', directory: false}
  const threadFile = {path: '/home/sessions/Plan-c3fa5c5a/plan.md', name: 'plan.md', directory: false}
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'files', context: {threadId: 'draft', label: 'New agent'}, threadId: 'draft', onselect}})
  await view.rerender({props: {requestedFile: draftFile}})
  expect(onselect).toHaveBeenLastCalledWith(`file:${draftFile.path}`)
  await view.rerender({props: {selected: `file:${draftFile.path}`, context: {threadId: 't2', threadTitle: 'Plan'}, threadId: 't2'}})
  await view.rerender({props: {requestedFile: threadFile}})
  await view.rerender({props: {selected: `file:${threadFile.path}`}})
  expect(screen.getByRole('tab', {name: 'live37-draft.txt'})).toBeInTheDocument()
  expect(screen.getByRole('tab', {name: 'Plan'})).toBeInTheDocument()
  await view.rerender({props: {gone: {ids: ['draft']}}})
  await waitFor(() => expect(screen.queryByRole('tab', {name: 'live37-draft.txt'})).toBeNull())
  expect(screen.getByRole('tab', {name: 'plan.md'})).toHaveAttribute('aria-selected', 'true')
  expect(screen.getByRole('tab', {name: 'Plan'})).toBeInTheDocument()
  expect(invoke.mock.calls.filter(([command, args]) => command === 'workspace_folders' && args.threadId === 't2')).toHaveLength(0)
  vi.unstubAllGlobals()
})
// A draft's file tab that closed while Agents put the panel aside comes back
// as the selection. The thread then shows its Files tab, not a blank panel.
it('selects the Files tab when the selected file tab closed while the panel was put aside', async () => {
  vi.stubGlobal('ResizeObserver', class {observe() {} disconnect() {}})
  vi.stubGlobal('matchMedia', () => ({matches: false, addEventListener() {}, removeEventListener() {}}))
  const invoke = vi.fn(async (command, args) => command === 'workspace_folders' ? [{label: 'Thread workspace', path: `/home/sessions/${args.threadId}`}] : command === 'workspace_list' ? {path: args.path, entries: []} : '')
  const onselect = vi.fn()
  const draftFile = {path: '/home/sessions/New_agent-0b3e9d21/live38-draft.txt', name: 'live38-draft.txt', directory: false}
  const view = render(WorkspacePanel, {props: {tauri: {invoke}, selected: 'files', context: {threadId: 'draft', label: 'New agent'}, threadId: 'draft', onselect}})
  await view.rerender({props: {requestedFile: draftFile}})
  await view.rerender({props: {selected: `file:${draftFile.path}`, requestedFile: null}})
  // Agents shows: the panel is put aside, then the draft goes.
  await view.rerender({props: {selected: null}})
  await view.rerender({props: {gone: {ids: ['draft']}}})
  await waitFor(() => expect(screen.queryByRole('tab', {name: 'live38-draft.txt'})).toBeNull())
  onselect.mockClear()
  // The next thread brings the put-aside selection back.
  await view.rerender({props: {selected: `file:${draftFile.path}`, context: {threadId: 't2', threadTitle: 'Plan'}, threadId: 't2'}})
  await waitFor(() => expect(onselect).toHaveBeenLastCalledWith('files'))
  await view.rerender({props: {selected: 'files'}})
  expect(screen.getByRole('tab', {name: 'Plan'})).toHaveAttribute('aria-selected', 'true')
  vi.unstubAllGlobals()
})

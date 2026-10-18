import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import NotificationsPage from './NotificationsPage.svelte'
import SidebarActions from '../lib/SidebarActions.svelte'
import { DEFAULT_NOTIFY, isNotificationsShortcut, notifyAllowed, notifyPreferences, resultLine, triggerLabel } from './notifications.js'

afterEach(cleanup)

const at = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString()
const run = (id, status, extra = {}) => ({ id, agent_id: 'scout', thread_id: `thread-${id}`, trigger: { kind: 'cron' }, status, created_at: at(5), updated_at: at(4), result_summary: null, silent: false, error: null, ...extra })
const local = { id: 'local', name: 'This computer' }
const page = (extra = {}) => ({
  needs_you: [
    { kind: 'input', id: 'r1', machine: local, agent_id: 'scout', agent_name: 'Lead scout', run: run('r1', 'waiting_input'), detail: 'Which total?', at: at(3) },
    { kind: 'limit', id: 'r2', machine: local, agent_id: 'desk', agent_name: 'Front desk', run: run('r2', 'failed'), resets_at_ms: Date.now() + 47 * 60_000, at: at(10) },
    { kind: 'offline', id: 'box', machine: { id: 'box', name: 'Build box' }, at: '' },
  ],
  running: [{ status: 'running', machine: local, agent_name: 'Lead scout', run: run('r3', 'running'), unread: false }],
  days: [{ day: 'today', entries: [
    { status: 'done', machine: local, agent_name: 'Lead scout', run: run('r4', 'completed', { result_summary: 'Found 4 leads.' }), unread: true },
    { status: 'failed', machine: local, agent_name: 'Lead scout', run: run('r5', 'failed', { error: 'The log was missing.', trigger: { kind: 'webhook' } }), unread: false },
  ] }],
  upcoming: [],
  counts: { needs_you: 2, running: 1, failed: 1, unread: 1 },
  machines: [local],
  agents: [{ id: 'scout', name: 'Lead scout' }],
  read_at: at(60),
  ...extra,
})

describe('notification words', () => {
  it('names triggers and results', () => {
    expect(triggerLabel({ kind: 'cron' })).toBe('Schedule')
    expect(triggerLabel({ kind: 'mcp' })).toBe('MCP')
    expect(resultLine({ status: 'failed', run: { error: 'Boom.' } })).toBe('Boom.')
    expect(resultLine({ status: 'done', run: { silent: true } })).toBe('Nothing needed a reply.')
  })

  it('notifies for what needs the user and, by choice, what finished', () => {
    expect(DEFAULT_NOTIFY).toEqual({ needs_you: true, finished: false })
    expect(notifyPreferences({ notify: { finished: true } })).toEqual({ needs_you: true, finished: true })
    expect(notifyAllowed({ status: 'completed' })).toBe(false)
    expect(notifyAllowed({ status: 'failed' })).toBe(true)
    expect(notifyAllowed({ status: 'completed' }, { needs_you: false, finished: true })).toBe(true)
    expect(notifyAllowed({ status: 'waiting_input' }, { needs_you: false, finished: true })).toBe(false)
  })

  it('takes Super+Shift+A', () => {
    expect(isNotificationsShortcut({ key: 'A', metaKey: true, shiftKey: true }, 'MacIntel')).toBe(true)
    expect(isNotificationsShortcut({ key: 'a', ctrlKey: true, shiftKey: true }, 'Linux x86_64')).toBe(true)
    expect(isNotificationsShortcut({ key: 'a', metaKey: true }, 'MacIntel')).toBe(false)
  })
})

describe('Notifications page', () => {
  it('pins what needs the user, then runs in flight, then the day', async () => {
    const invoke = vi.fn(async (command) => command === 'notifications_list' ? page() : {})
    const onopen = vi.fn()
    render(NotificationsPage, { tauri: { invoke }, runtimeList: { runtimes: [{ id: 'local', local: true }] }, onopen })
    const needs = await screen.findByRole('region', { name: /^Needs you/ })
    expect(invoke).toHaveBeenCalledWith('notifications_list', { machines: [local], filter: expect.objectContaining({ view: 'all' }) })
    expect(within(needs).getByRole('article', { name: 'Answer needed: Lead scout' })).toBeInTheDocument()
    expect(within(needs).getByRole('article', { name: 'Usage limit reached: Front desk' })).toHaveTextContent('Resets in')
    expect(within(needs).getByRole('button', { name: 'Run again at reset' })).toBeDisabled()
    expect(within(needs).getByRole('article', { name: 'Machine offline: Build box' })).toBeInTheDocument()
    expect(screen.getByRole('list', { name: 'Running now' })).toHaveTextContent('Lead scout')
    const today = screen.getByRole('region', { name: 'Today' })
    await fireEvent.click(within(today).getByRole('button', { name: 'Lead scout, Done, unread: Found 4 leads.' }))
    expect(onopen).toHaveBeenCalledWith(expect.objectContaining({ id: 'r4', thread_id: 'thread-r4', runtimeId: 'local' }))
  })

  it('posts a reply to a waiting run and marks everything read', async () => {
    const invoke = vi.fn(async (command) => command === 'notifications_list' ? page() : {})
    const onread = vi.fn()
    render(NotificationsPage, { tauri: { invoke }, runtimeList: null, onread })
    const reply = await screen.findByRole('textbox', { name: 'Reply to Lead scout' })
    await fireEvent.input(reply, { target: { value: 'Use the PDF total.' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'thread.post', body: { thread_id: 'thread-r1', text: 'Use the PDF total.' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Mark all read' }))
    await vi.waitFor(() => expect(onread).toHaveBeenCalled())
    expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'preferences.set', body: { values: { 'notifications-read-at': expect.any(String) } } })
  })

  it('says nothing needs the user and lists the next scheduled runs', async () => {
    const quiet = page({ needs_you: [], running: [], upcoming: [{ agent_id: 'scout', agent_name: 'Lead scout', routine_id: 'daily', routine_name: 'Weekday briefing', at_ms: Date.now() + 3_600_000 }] })
    render(NotificationsPage, { tauri: { invoke: vi.fn(async () => quiet) }, runtimeList: null })
    expect(await screen.findByText('Nothing needs you')).toBeInTheDocument()
    expect(screen.getByRole('list', { name: 'Next scheduled runs' })).toHaveTextContent('Weekday briefing')
  })

  it('asks again for the failed view', async () => {
    const invoke = vi.fn(async () => page())
    render(NotificationsPage, { tauri: { invoke }, runtimeList: null })
    await screen.findByRole('region', { name: /^Needs you/ })
    await fireEvent.click(screen.getByRole('radio', { name: 'Failed' }))
    await vi.waitFor(() => expect(invoke).toHaveBeenLastCalledWith('notifications_list', expect.objectContaining({ filter: expect.objectContaining({ view: 'failed' }) })))
  })
})

describe('sidebar actions', () => {
  it('names each icon and counts only what needs the user', async () => {
    const onusage = vi.fn()
    render(SidebarActions, { needsYou: 3, usageShortcut: 'Meta+Shift+U', notificationsShortcut: 'Meta+Shift+A', settingsShortcut: 'Meta+,', onsettings: vi.fn(), onusage, onnotifications: vi.fn() })
    expect(screen.getByRole('button', { name: 'Settings' })).toHaveAttribute('aria-keyshortcuts', 'Meta+,')
    expect(screen.getByRole('button', { name: 'Notifications, 3 need you' })).toHaveTextContent('3')
    await fireEvent.click(screen.getByRole('button', { name: 'Usage' }))
    expect(onusage).toHaveBeenCalledOnce()
  })
})

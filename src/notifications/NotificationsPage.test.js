import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import NotificationsPage from './NotificationsPage.svelte'
import SidebarActions from '../lib/SidebarActions.svelte'
import { DEFAULT_NOTIFY, isNotificationsShortcut, notifyAllowed, notifyPreferences, resultLine, triggerIcon, triggerLabel } from './notifications.js'
import { machineLight } from '../lib/status-tone.js'

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

  it('gives each trigger the routine rows\' icon and a manual run the Run now play', () => {
    expect(['cron', 'webhook', 'mcp', 'manual', 'agent_message', 'other'].map((kind) => triggerIcon({ kind })))
      .toEqual(['clock', 'link', 'plug-zap', 'play', 'message-square', 'zap'])
  })

  it('previews a Markdown result as plain text', () => {
    const summary = '- **Tauri 2.12 raises compatibility requirements.** Released with [notes](https://example.com) and `cargo` fixes.\n- Second _item_'
    expect(resultLine({ status: 'done', run: { result_summary: summary } })).toBe('Tauri 2.12 raises compatibility requirements. Released with notes and cargo fixes. Second item')
    expect(resultLine({ status: 'needs_you', run: { result_summary: '## Which total?' } })).toBe('Which total?')
  })

  it('lights a machine green while it answers, yellow while it connects and red otherwise', () => {
    expect([{ local: true }, { status: 'online' }, { status: 'connecting' }, { status: 'unknown' }, { status: 'offline' }, { status: 'revoked' }, null].map(machineLight))
      .toEqual(['green', 'green', 'yellow', 'yellow', 'red', 'red', null])
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

  it('names each run\'s trigger with its icon and its machine with its stoplight', async () => {
    const box = { id: 'box', name: 'Build box' }
    const runs = page({ needs_you: [], running: [], days: [{ day: 'today', entries: [
      { status: 'done', machine: local, agent_name: 'Lead scout', run: run('r4', 'completed', { result_summary: 'Found **4** leads.' }), unread: false },
      { status: 'failed', machine: box, agent_name: 'Builder', run: run('r5', 'failed', { error: 'No answer.', trigger: { kind: 'webhook' } }), unread: false },
    ] }], upcoming: [{ agent_id: 'scout', agent_name: 'Lead scout', routine_id: 'daily', routine_name: 'Weekday briefing', machine: box, at_ms: Date.now() + 3_600_000 }] })
    const runtimeList = { runtimes: [{ id: 'local', local: true, kind: 'local', formFactor: 'laptop', status: 'online' }, { id: 'studio', kind: 'desktop', status: 'online' }, { id: 'box', kind: 'container', status: 'offline' }] }
    render(NotificationsPage, { tauri: { invoke: vi.fn(async () => runs) }, runtimeList })
    const today = await screen.findByRole('region', { name: 'Today' })
    const done = within(today).getByRole('button', { name: 'Lead scout, Done, on This computer, Online: Found 4 leads.' })
    expect(done.querySelector('.chip [data-icon="clock"]')).not.toBeNull()
    expect(done.querySelector('.machine-chip')).toHaveAttribute('data-light', 'green')
    expect(done.querySelector('.machine-chip [data-icon="laptop"]')).not.toBeNull()
    expect(done.querySelector('.machine-chip')).toHaveAttribute('data-tooltip', 'Online')
    const failed = within(today).getByRole('button', { name: 'Builder, Failed, on Build box, Offline: No answer.' })
    expect(failed.querySelector('.chip [data-icon="link"]')).not.toBeNull()
    expect(failed.querySelector('.machine-chip')).toHaveAttribute('data-light', 'red')
    expect(failed.querySelector('.machine-chip [data-icon="container"]')).not.toBeNull()
    const upcoming = screen.getByRole('list', { name: 'Next scheduled runs' })
    expect(upcoming.querySelector('.machine-chip')).toHaveAttribute('data-light', 'red')
    expect(upcoming).toHaveTextContent('Build box, Offline')
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

  it('shows one name at a time and hides it when the pointer or focus leaves', async () => {
    vi.useFakeTimers()
    try {
      const view = render(SidebarActions, { usageShortcut: 'Meta+Shift+U', notificationsShortcut: 'Meta+Shift+A', settingsShortcut: 'Meta+,', onsettings: vi.fn(), onusage: vi.fn(), onnotifications: vi.fn() })
      const settings = screen.getByRole('button', { name: 'Settings' })
      const usage = screen.getByRole('button', { name: 'Usage' })
      // Focus the app moves back after a dialog closes names nothing.
      settings.focus()
      await vi.advanceTimersByTimeAsync(500)
      expect(screen.queryByRole('tooltip')).toBeNull()
      await fireEvent.pointerEnter(settings)
      await vi.advanceTimersByTimeAsync(400)
      expect(screen.getByRole('tooltip')).toHaveTextContent(/^Settings/)
      await fireEvent.pointerLeave(settings)
      await fireEvent.pointerEnter(usage)
      expect(screen.queryByRole('tooltip')).toBeNull()
      await vi.advanceTimersByTimeAsync(400)
      expect(screen.getAllByRole('tooltip')).toHaveLength(1)
      expect(screen.getByRole('tooltip')).toHaveTextContent(/^Usage/)
      await fireEvent.pointerLeave(usage)
      expect(screen.queryByRole('tooltip')).toBeNull()
      // Keyboard focus names its button, and blur or a page change hides it.
      await fireEvent.keyDown(window, { key: 'Tab' })
      await fireEvent.focus(usage)
      expect(screen.getByRole('tooltip')).toHaveTextContent(/^Usage/)
      await view.rerender({ usageOpen: true })
      expect(screen.queryByRole('tooltip')).toBeNull()
      await fireEvent.focus(usage)
      await fireEvent.blur(usage)
      expect(screen.queryByRole('tooltip')).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })
})

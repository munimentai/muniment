import { afterEach, expect, it, vi } from 'vitest'
import { userEvent } from 'vitest/browser'
import { flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import App from '../App.svelte'
import StatusRing from './StatusRing.svelte'
import { arrangeStandIn } from '../../test/sidebar-standin.js'

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({ register: async () => {}, unregister: async () => {} }))
vi.mock('@tauri-apps/api/window', () => ({
  Window: { getByLabel: async () => null },
  UserAttentionType: { Informational: 2 },
  getCurrentWindow: () => ({ onMoved: async () => () => {}, onResized: async () => () => {}, isFocused: async () => true, requestUserAttention: async () => {} }),
}))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }))

// The window mounted on a stand-in host: three threads with long titles, a
// running one among them, on this computer alone or with a paired build box.
const LONG = 'Quarterly vendor contract renewal review and follow-up notes'
const at = new Date(Date.now() - 5 * 60_000).toISOString()
const local = { id: 'local', name: 'This computer', kind: 'local', version: '0.2.0', status: 'online', local: true, scopes: [], admin: true, updateOffered: false, sshHost: null, active: true }
const box = { id: '018f0000-0000-7000-8000-000000000043', name: 'Build box', kind: 'container', version: '0.2.0', status: 'online', local: false, scopes: ['run.write', 'thread.read'], admin: false, updateOffered: false, sshHost: null, active: false }

let app
afterEach(() => {
  if (app) unmount(app)
  app = null
  delete window.__TAURI__
  delete document.documentElement.dataset.motion
  document.body.replaceChildren()
})

async function mountWindow(paired) {
  const summaries = [
    { threadId: 'open', title: 'Open thread', updatedAt: at, runtimeId: 'local' },
    { threadId: 'long', title: LONG, updatedAt: at, runtimeId: paired ? box.id : 'local' },
    { threadId: 'busy', title: `${LONG} again`, updatedAt: at, runtimeId: 'local' },
  ]
  const answers = {
    auth_status: { signed_in: true, subject: 'probe' },
    home_status: { configured: true, homePath: '/Documents/Muniment' },
    runtime_state: { revision: 0, lastEvent: 'connected', visible: false, busy: false },
    runtime_list: { appVersion: '0.2.0', deviceName: 'Laptop', selected: 'local', runtimes: paired ? [local, box] : [local] },
    chat_thread_summaries: { summaries, nextCursor: null },
    chat_thread_open: { entries: [], nextCursor: null },
    project_list: { projects: {}, threads: {} },
    extend_command: { items: [], threads: {} },
    local_mode_status: false,
    attach_listener_status: { started: true, failure: null, connected: true },
  }
  window.__TAURI__ = {
    core: { invoke: async (command, payload) => {
      if (command === 'sidebar_arrange') {
        const sidebar = arrangeStandIn(payload?.input)
        for (const project of sidebar.projects) for (const row of project.threads) if (row.thread_id === 'busy') Object.assign(row, { status: 'working', status_label: 'Working' })
        return sidebar
      }
      if (command in answers) return structuredClone(answers[command])
      return null
    } },
    event: { listen: async () => () => {}, emitTo: async () => {} },
  }
  const target = document.createElement('div')
  document.body.append(target)
  app = mount(App, { target })
  flushSync()
  await expect.poll(() => document.querySelectorAll('.thread-record button.thread-row').length, { timeout: 5000 }).toBe(2)
}

const record = (id) => document.querySelector(`.thread-row[data-thread-id="${id}"]`).closest('.thread-record')
const rect = (element) => element.getBoundingClientRect()

// Hovers the row and waits for its actions control to show.
async function hover(id) {
  const row = record(id)
  await userEvent.hover(row.querySelector('.thread-row-title'))
  await expect.poll(() => getComputedStyle(row.querySelector('.thread-actions')).opacity).toBe('1')
  return row
}

it('keeps the actions control clear of a long title on this computer alone, in the time\'s place', async () => {
  await mountWindow(false)
  for (const id of ['long', 'busy']) {
    const row = record(id)
    const title = row.querySelector('.thread-row-title')
    const time = row.querySelector('time')
    expect(row.querySelector('.thread-row-meta')).toBeNull()
    // Title and time share one line.
    expect(Math.abs(rect(title).top + rect(title).height / 2 - (rect(time).top + rect(time).height / 2))).toBeLessThan(2)
    const before = rect(title).right
    await hover(id)
    const actions = row.querySelector('.thread-actions')
    expect(getComputedStyle(time).visibility).toBe('hidden')
    expect(rect(actions).left).toBeGreaterThanOrEqual(rect(time).left)
    expect(rect(title).right).toBeLessThan(rect(actions).left)
    expect(rect(title).right).toBe(before)
    await userEvent.unhover(row)
  }
  // The running row's ring sits left of its title.
  const busy = record('busy')
  expect(rect(busy.querySelector('.status-ring')).right).toBeLessThanOrEqual(rect(busy.querySelector('.thread-row-title')).left)
})

it('keeps the actions control clear of a long title under a meta line, and the time in place', async () => {
  await mountWindow(true)
  for (const id of ['long', 'busy']) {
    const row = record(id)
    const title = row.querySelector('.thread-row-title')
    const time = row.querySelector('.thread-row-meta time')
    expect(title.dataset.overflow).toBe('true')
    const timeBefore = rect(time)
    await hover(id)
    const actions = row.querySelector('.thread-actions')
    // The title's box, and the fade at its end, stop before the control.
    expect(rect(title).right).toBeLessThan(rect(actions).left)
    expect(rect(time).left).toBe(timeBefore.left)
    expect(rect(time).top).toBe(timeBefore.top)
    expect(getComputedStyle(time).visibility).toBe('visible')
    await userEvent.unhover(row)
  }
  // The ring takes the machine icon's place on the meta line.
  const meta = record('busy').querySelector('.thread-row-meta')
  expect(meta.querySelector('.status-ring')).not.toBeNull()
  expect(meta.querySelector('[data-icon="monitor"], [data-icon="laptop"]')).toBeNull()
})

it('keeps the ring traveling and the slot sliding under reduced motion', () => {
  document.documentElement.dataset.motion = 'reduce'
  const target = document.createElement('div')
  document.body.append(target)
  app = mount(StatusRing, { target, props: { status: 'working', label: 'Working', key: 'motion' } })
  flushSync()
  const dashes = [...target.querySelectorAll('.status-ring path')]
  expect(dashes).toHaveLength(8)
  for (const dash of dashes) {
    expect(getComputedStyle(dash).animationName).toMatch(/ring-travel$/)
    expect(getComputedStyle(dash).animationDuration).toBe('1.2s')
    expect(getComputedStyle(dash).animationIterationCount).toBe('infinite')
  }
  // Each dash starts a step later around the circle.
  expect(dashes.map((dash) => getComputedStyle(dash).animationDelay)).toEqual(['0s', '-1.05s', '-0.9s', '-0.75s', '-0.6s', '-0.45s', '-0.3s', '-0.15s'])
  expect(getComputedStyle(target.querySelector('.status-lead')).transitionDuration).toBe('0.16s')
})

it('draws each tone in its status color', () => {
  for (const [status, token] of [['working', '--success'], ['needs_you', '--warning'], ['failed', '--danger']]) {
    const target = document.createElement('div')
    document.body.append(target)
    const ring = mount(StatusRing, { target, props: { status, label: status, key: `tone-${status}` } })
    flushSync()
    const probe = document.createElement('i')
    probe.style.color = `var(${token})`
    target.append(probe)
    expect(getComputedStyle(target.querySelector('.status-ring path')).stroke).toBe(getComputedStyle(probe).color)
    unmount(ring)
  }
})

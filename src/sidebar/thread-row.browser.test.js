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

it('marks the Agents and Artifacts rows as the open row while their page shows, under the pointer too', async () => {
  await mountWindow(false)
  const sidebar = document.getElementById('sidebar')
  const fill = (element) => getComputedStyle(element).backgroundColor
  const probe = document.createElement('i')
  probe.style.backgroundColor = 'var(--selected)'
  document.body.append(probe)
  for (const name of ['Agents', 'Artifacts']) {
    const row = sidebar.querySelector(`button[aria-label="${name}"]`)
    expect(row).not.toHaveAttribute('aria-current')
    expect(getComputedStyle(row, '::before').content).toBe('none')
    await userEvent.click(row)
    await expect.poll(() => row.getAttribute('aria-current')).toBe('true')
    // The pointer still rests on the row it pressed.
    expect(fill(row)).toBe(fill(probe))
    await userEvent.unhover(row)
    await userEvent.hover(document.querySelector('.catalog-title'))
    expect(fill(row)).toBe(fill(probe))
    // The accent edge runs down its left side, as on an open thread's row.
    const edge = getComputedStyle(row, '::before')
    expect(edge.position).toBe('absolute')
    expect(edge.width).toBe('2px')
    for (const other of ['Agents', 'Artifacts'].filter((entry) => entry !== name)) expect(sidebar.querySelector(`button[aria-label="${other}"]`)).not.toHaveAttribute('aria-current')
  }
})

// Starts a new thread and sends its first message, whose reply stays running.
async function sendFirstMessage(text) {
  const core = window.__TAURI__.core.invoke
  window.__TAURI__.core.invoke = async (command, payload) => {
    if (command === 'chat_new_thread') return 'fresh-1'
    if (command === 'chat_submit') return { runId: 'run-fresh', attachments: [] }
    if (command === 'chat_current_thread') return 'fresh-1'
    return core(command, payload)
  }
  await userEvent.click(document.querySelector('button[aria-label="New thread"]'))
  await expect.poll(() => document.querySelector('textarea')).toBeTruthy()
  await userEvent.click(document.querySelector('textarea'))
  await userEvent.keyboard(text)
  await userEvent.keyboard('{Enter}')
  await expect.poll(() => document.querySelector('[data-fresh-thread]')).toBeTruthy()
  return document.querySelector('[data-fresh-thread]')
}

it('ends a new thread row\'s long title in an ellipsis and names its machine and time while the first reply runs', async () => {
  await mountWindow(true)
  const row = await sendFirstMessage(`Plan a starship tour for a ${LONG}`)
  const title = row.querySelector('.thread-row-title')
  expect(getComputedStyle(title).textOverflow).toBe('ellipsis')
  expect(title.scrollWidth).toBeGreaterThan(title.clientWidth)
  // The meta line names the machine and the time, as a listed row's does.
  const meta = row.querySelector('.thread-row-meta')
  expect(meta).not.toBeNull()
  expect(meta.textContent).toContain('This computer')
  expect(meta.querySelector('time').textContent).toBe('now')
  expect(row.querySelector('.status-ring')).not.toBeNull()
  expect(rect(title).right).toBeLessThanOrEqual(rect(row).right)
})

it('keeps a new thread row on one line with its time on this computer alone', async () => {
  await mountWindow(false)
  const row = await sendFirstMessage(`Plan a starship tour for a ${LONG}`)
  const title = row.querySelector('.thread-row-title')
  expect(getComputedStyle(title).textOverflow).toBe('ellipsis')
  expect(row.querySelector('.thread-row-meta')).toBeNull()
  const time = row.querySelector('time')
  expect(time.textContent).toBe('now')
  expect(Math.abs(rect(title).top + rect(title).height / 2 - (rect(time).top + rect(time).height / 2))).toBeLessThan(2)
  expect(rect(title).right).toBeLessThanOrEqual(rect(time).left)
})

it('shows the start of a long new thread title in the title pill and ends it in an ellipsis', async () => {
  await mountWindow(false)
  const text = `Plan a starship tour for a ${LONG}`
  await sendFirstMessage(text)
  const pill = document.querySelector('.titlebar .thread-title')
  await expect.poll(() => pill.textContent.trim().startsWith('Plan a starship tour')).toBe(true)
  const style = getComputedStyle(pill)
  expect(style.textOverflow).toBe('ellipsis')
  expect(pill.scrollWidth).toBeGreaterThan(pill.clientWidth)
  // The first character sits at the pill's start, inside its box.
  const range = document.createRange()
  range.setStart(pill.firstChild, 0)
  range.setEnd(pill.firstChild, 1)
  const first = range.getBoundingClientRect()
  expect(first.left).toBeGreaterThanOrEqual(rect(pill).left)
  expect(first.left - rect(pill).left).toBeLessThanOrEqual(parseFloat(style.paddingLeft) + parseFloat(style.borderLeftWidth) + 0.5)
})

it('ends the Agents and Artifacts labels in an ellipsis when the row is narrow', async () => {
  await mountWindow(false)
  for (const name of ['Agents', 'Artifacts']) {
    const row = document.querySelector(`#sidebar button[aria-label="${name}"]`)
    row.style.width = '56px'
    const label = row.querySelector('span')
    expect(label.scrollWidth).toBeGreaterThan(label.clientWidth)
    expect(getComputedStyle(label).textOverflow).toBe('ellipsis')
    expect(getComputedStyle(label).maskImage).toBe('none')
  }
})

// The disabled title-bar actions control names why it waits, on hover and on
// focus, and its reason draws over the thread panel under it.
it('draws the reason on the disabled title thread actions over the thread panel, on hover and on focus', async () => {
  await mountWindow(false)
  const core = window.__TAURI__.core.invoke
  window.__TAURI__.core.invoke = async (command, payload) => command === 'chat_new_thread' ? 'fresh-1' : command === 'chat_current_thread' ? 'fresh-1' : core(command, payload)
  await userEvent.click(document.querySelector('button[aria-label="New thread"]'))
  await expect.poll(() => document.querySelector('.title-thread-actions[aria-disabled="true"]')).toBeTruthy()
  const control = document.querySelector('.title-thread-actions')
  expect(control.dataset.tooltip).toBe('Send a message to use thread actions.')
  const tip = () => getComputedStyle(control, '::after')
  // A hit test finds what paints on top at the reason's place, so the probe
  // lets the reason take the pointer.
  const probe = document.createElement('style')
  probe.textContent = '.title-thread-actions::after { pointer-events: auto !important; }'
  document.head.append(probe)
  try {
    await userEvent.hover(control)
    await expect.poll(() => tip().opacity).toBe('1')
    expect(getComputedStyle(control).opacity).toBe('1')
    const box = rect(control)
    const at = document.elementFromPoint(box.left + 12, box.bottom + 6 + parseFloat(tip().height) / 2)
    expect(at).toBe(control)
    await userEvent.unhover(control)
    await expect.poll(() => tip().opacity).toBe('0')
    control.focus({ focusVisible: true })
    await userEvent.keyboard('{Shift>}{Tab}{/Shift}{Tab}')
    await expect.poll(() => document.activeElement).toBe(control)
    await expect.poll(() => tip().opacity).toBe('1')
  } finally {
    probe.remove()
  }
})

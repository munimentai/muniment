import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import '../styles/scrollbars.css'
import NotificationsPage from './NotificationsPage.svelte'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const local = { id: 'local', name: 'This computer' }
const at = new Date().toISOString()
const entry = (id) => ({ status: 'done', machine: local, agent_name: 'Scout', run: { id, agent_id: 'scout', thread_id: `t-${id}`, trigger: { kind: 'cron' }, status: 'completed', created_at: at, updated_at: at, result_summary: 'Done.', silent: false, error: null }, unread: false })
// All lists more runs than the page shows, so it scrolls. Needs you lists none.
const page = (view) => ({
  needs_you: [], running: [], upcoming: [],
  days: view === 'all' ? [{ day: 'today', entries: Array.from({ length: 40 }, (_, index) => entry(`r${index}`)) }] : [],
  counts: { needs_you: 0, running: 0, failed: 0, unread: 0 }, machines: [local], agents: [], read_at: at,
})

// Headless Chromium hides scrollbars, so the shift shows in the WebKit run.
it('keeps the Show filters in place when a filter empties the page and its scrollbar goes', async () => {
  const tauri = { invoke: async (command, args) => command === 'notifications_list' ? page(args.filter.view) : {} }
  const target = document.createElement('div')
  Object.assign(target.style, { display: 'flex', flexDirection: 'column', height: '500px', width: '700px' })
  document.body.append(target)
  component = mount(NotificationsPage, { target, props: { tauri, runtimeList: { runtimes: [{ id: 'local', local: true }] } } })
  await wait(200)
  const radio = (name) => [...document.querySelectorAll('[role="radio"]')].find((node) => node.textContent === name)
  const edges = () => ['Needs you', 'All', 'Failed'].map((name) => radio(name).getBoundingClientRect()).map((box) => [box.left, box.width])
  const scroller = document.querySelector('.panel-page')
  expect(scroller.scrollHeight).toBeGreaterThan(scroller.clientHeight)
  const before = edges()
  await userEvent.click(radio('Needs you'))
  await wait(200)
  expect(scroller.scrollHeight).toBeLessThanOrEqual(scroller.clientHeight)
  expect(edges()).toEqual(before)
})

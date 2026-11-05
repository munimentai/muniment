import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import Settings from './Settings.svelte'
import { trackFocusSource } from './focus-source.js'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const key = { id: 'k1', label: 'Jev', family: 'openai', source: 'key', models: [] }
const inventory = { providers: [{ id: 'openai', name: 'OpenAI', source: 'key', family: 'openai', base_url: null, models: [{ id: 'gpt-5.5' }] }], hidden: [], shown: [], default_provider: null, default_model: null }
const router = { families: [{ id: 'openai', name: 'OpenAI' }], subscriptions: [], accounts: [key], classifier_connections: [{ id: 'jev', name: 'Jev · TypeSafe API', catalog_id: 'openai', active: true, connection: { kind: 'key' } }] }

// Settings › Models as the app shows it, with its Escape handler beneath the dialog.
// `frame` sets it beside a sidebar that, as the app's sidebar divider does,
// stacks over the Settings panel.
async function models({ frame = false } = {}) {
  const calls = []
  const tauri = { invoke: async (command, args) => {
    calls.push([command, args])
    if (command === 'model_router_settings') return router
    if (command === 'local_mode_provider_inventory') return inventory
    return null
  } }
  let closed = 0
  const target = document.createElement('div')
  document.body.append(target)
  let sidebarClicks = 0
  if (frame) {
    target.style.cssText = 'position: fixed; inset: 0; display: grid; grid-template-columns: 200px 1fr; grid-template-rows: 36px 1fr;'
    const sidebar = document.createElement('nav')
    sidebar.style.cssText = 'grid-row: 1 / -1; grid-column: 1; position: relative; z-index: 2;'
    sidebar.addEventListener('click', () => { sidebarClicks += 1 })
    target.append(sidebar)
  }
  component = mount(Settings, { target, props: { tauri, listen: async () => () => {}, inventory, section: 'models', onclose: () => { closed += 1 } } })
  // The first render in a cold webkit worker compiles Settings, which takes seconds on CI.
  const end = performance.now() + 15000
  while (!document.querySelector('[aria-label="Disconnect Jev"]') || !document.querySelector('[aria-label="Disconnect Jev · TypeSafe API"]')) {
    if (performance.now() > end) throw new Error('Settings › Models did not render both Disconnect buttons')
    await wait(30)
  }
  return { calls, closed: () => closed, sidebarClicks: () => sidebarClicks }
}
const confirm = () => document.querySelector('[data-panel="confirm"]')
const button = (name) => [...confirm().querySelectorAll('button')].find((node) => node.textContent === name)

it('closes the decision-model Disconnect dialog on Cancel and on Escape, wherever focus sits, and keeps Settings open', async () => {
  const page = await models()
  const unplug = document.querySelector('[aria-label="Disconnect Jev · TypeSafe API"]')
  await userEvent.click(unplug)
  await wait(50)
  expect(document.activeElement).toBe(button('Cancel'))
  await userEvent.click(button('Cancel'))
  await wait(50)
  expect(confirm()).toBeNull()

  await userEvent.click(unplug)
  await wait(50)
  // A click on the dialog's text leaves focus outside its buttons.
  document.activeElement.blur()
  await userEvent.keyboard('{Escape}')
  await wait(50)
  expect(confirm()).toBeNull()
  expect(page.closed()).toBe(0)
  expect(page.calls.some(([command]) => command === 'model_router_disconnect_classifier')).toBe(false)
})

const ringed = (node) => getComputedStyle(node).outlineStyle === 'solid' && getComputedStyle(node).outlineWidth === '1px'

it('returns focus to the Disconnect button that opened the dialog, with the ring when the keys lead', async () => {
  const stop = trackFocusSource(document.documentElement)
  try {
    await models()
    const unplug = document.querySelector('[aria-label="Disconnect Jev · TypeSafe API"]')
    // By pointer: focus returns to the button, which a click draws no ring on.
    await userEvent.click(unplug)
    await wait(50)
    await userEvent.click(button('Cancel'))
    await wait(50)
    expect(confirm()).toBeNull()
    expect(document.activeElement).toBe(unplug)
    expect(ringed(unplug)).toBe(false)
    // By keys: Cancel takes focus with the ring, and Enter on it returns focus, ring and all.
    await userEvent.keyboard('{Enter}')
    await wait(50)
    expect(document.activeElement).toBe(button('Cancel'))
    expect(ringed(button('Cancel'))).toBe(true)
    await userEvent.keyboard('{Enter}')
    await wait(50)
    expect(confirm()).toBeNull()
    expect(document.activeElement).toBe(unplug)
    expect(ringed(unplug)).toBe(true)
  } finally { stop(); delete document.documentElement.dataset.pointer }
})

// A real pointer reaches the dialog's own controls and backdrop wherever the
// page sits: the decision model's row lies at the foot of a scrolled page, and
// the app's sidebar stacks over the Settings panel. AXPress and Escape never hit-test.
it('takes a real pointer click on Cancel and on the backdrop over the sidebar for the decision-model dialog', async () => {
  const page = await models({ frame: true })
  const unplug = document.querySelector('[aria-label="Disconnect Jev · TypeSafe API"]')
  unplug.scrollIntoView()
  await userEvent.click(unplug)
  await wait(50)
  // The dialog leaves the scrolled Settings panel for the page body.
  expect(document.querySelector('[data-panel="settings"]').contains(confirm())).toBe(false)
  const cancel = button('Cancel').getBoundingClientRect()
  expect(document.elementFromPoint(cancel.x + cancel.width / 2, cancel.y + cancel.height / 2)).toBe(button('Cancel'))
  await userEvent.click(button('Cancel'))
  await wait(50)
  expect(confirm()).toBeNull()

  await userEvent.click(unplug)
  await wait(50)
  // `force` presses the mouse at the point whatever lies on top, as a person does.
  await userEvent.click(document.querySelector('.dialog-backdrop'), { position: { x: 100, y: 60 }, force: true })
  await wait(50)
  expect(confirm()).toBeNull()
  expect(page.sidebarClicks()).toBe(0)
  expect(page.calls.some(([command]) => command === 'model_router_disconnect_classifier')).toBe(false)
})

it('marks Disconnect with the danger treatment in the account dialog as in the decision-model dialog', async () => {
  await models()
  for (const name of ['Disconnect Jev', 'Disconnect Jev · TypeSafe API']) {
    await userEvent.click(document.querySelector(`[aria-label="${name}"]`))
    await wait(50)
    const cancel = getComputedStyle(button('Cancel'))
    const disconnect = getComputedStyle(button('Disconnect'))
    const danger = getComputedStyle(document.documentElement).getPropertyValue('--danger').trim()
    expect(disconnect.backgroundColor).not.toBe(cancel.backgroundColor)
    expect(disconnect.color).not.toBe(cancel.color)
    const probe = document.createElement('span')
    probe.style.color = danger
    document.body.append(probe)
    expect(disconnect.color).toBe(getComputedStyle(probe).color)
    probe.remove()
    expect(document.activeElement).toBe(button('Cancel'))
    await userEvent.click(button('Cancel'))
    await wait(50)
  }
})

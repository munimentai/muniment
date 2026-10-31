import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import Settings from './Settings.svelte'

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
async function models() {
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
  component = mount(Settings, { target, props: { tauri, listen: async () => () => {}, inventory, section: 'models', onclose: () => { closed += 1 } } })
  const end = performance.now() + 4000
  while (!document.querySelector('[aria-label="Disconnect Jev"]') || !document.querySelector('[aria-label="Disconnect Jev · TypeSafe API"]')) {
    if (performance.now() > end) throw new Error('Settings › Models did not render both Disconnect buttons')
    await wait(30)
  }
  return { calls, closed: () => closed }
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

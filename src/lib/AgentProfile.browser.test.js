import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import AgentProfile from './AgentProfile.svelte'
import { RAIL_OCCUPANTS } from './artifact-rail-state.js'
import { trackFocusSource } from './focus-source.js'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

const agent = { id: 'scout', name: 'Scout', label: 'Researcher', instructions: 'Find sources.', projectId: 'p', runtimeId: 'local', routines: [], skills: [], permissions: { mode: 'manual' }, model: { provider: 'ollama', id: 'qwen3-coder-30b-a3b-instruct-q4' } }
const inventory = { providers: [{ id: 'ollama', name: 'Ollama', source: 'local', models: [{ id: 'qwen3-coder-30b-a3b-instruct-q4', thinking_levels: ['low', 'high'], thinking_default: 'low' }] }] }
const modes = {
  modes: [{ id: 'manual', title: 'Ask before changes', description: 'Edits ask first.', available: true }],
  default_thread_mode: 'manual',
  default_agent_mode: 'manual',
}
const tauri = {
  invoke: async (command, args) => {
    if (command !== 'agent_request') return null
    if (args.operation === 'permission.modes') return modes
    if (args.operation === 'agent.runs') return { runs: [], next_cursor: null }
    return {}
  },
}

// The profile in its rail column, `width` pixels wide and `height` tall.
function profile({ width = RAIL_OCCUPANTS.agent.min, height = 800, ...props } = {}) {
  const target = document.createElement('div')
  Object.assign(target.style, { position: 'absolute', top: '0', right: '0', width: `${width}px`, height: `${height}px`, display: 'grid', gridTemplateAreas: '"rail"', gridTemplateColumns: `${width}px`, gridTemplateRows: `${height}px` })
  document.body.append(target)
  component = mount(AgentProfile, { target, props: { agent, tauri, inventory, projects: [['p', 'Quarterly research archive']], onchange: async () => {}, ondelete: () => {}, ...props } })
  flushSync()
  return target.querySelector('aside')
}

it('keeps the Project, Model and Permissions rows inside the profile at its minimum width', async () => {
  const aside = profile()
  await expect.poll(() => aside.querySelector('[aria-label^="Permissions:"]')).toBeTruthy()
  const panel = aside.getBoundingClientRect()
  expect(Math.round(panel.width)).toBe(RAIL_OCCUPANTS.agent.min)
  for (const name of ['Project', 'Model', 'Permissions']) {
    const row = aside.querySelector(`.setting[aria-label="${name}"]`)
    const box = row.getBoundingClientRect()
    expect(row.scrollWidth, name).toBeLessThanOrEqual(row.clientWidth)
    expect(box.right, name).toBeLessThanOrEqual(panel.right)
    for (const control of row.querySelectorAll('button')) {
      const edge = control.getBoundingClientRect()
      expect(edge.left, name).toBeGreaterThanOrEqual(box.left - 0.5)
      expect(edge.right, name).toBeLessThanOrEqual(box.right + 0.5)
      // Its icon, label and chevrons sit inside it, and a long label ends in its ellipsis.
      for (const part of control.children) {
        const inner = part.getBoundingClientRect()
        if (!inner.width) continue
        expect(inner.left, `${name} ${part.className}`).toBeGreaterThanOrEqual(edge.left - 0.5)
        expect(inner.right, `${name} ${part.className}`).toBeLessThanOrEqual(edge.right + 0.5)
      }
      const label = control.querySelector('.label')
      if (label.scrollWidth > label.clientWidth) expect(getComputedStyle(label).textOverflow).toBe('ellipsis')
    }
  }
})

const reason = 'The model provider refused the request because the account has no credit left for this model run today.'

for (const [width, clamped] of [[RAIL_OCCUPANTS.agent.min, true], [RAIL_OCCUPANTS.agent.max, false]]) {
  it(`shows Show all for a ${reason.length + 17}-character reason only when the state line clips it, at ${width}px`, async () => {
    const aside = profile({ width, activity: { state: 'error', action: reason, since: null } })
    const line = [...aside.querySelectorAll('p.state')].find((node) => node.textContent === `Last run failed: ${reason}`)
    expect(line).toBeTruthy()
    expect(line.scrollHeight > line.clientHeight + 1).toBe(clamped)
    const more = () => aside.querySelector('.state-more')
    await expect.poll(() => !!more()).toBe(clamped)
    if (!clamped) return
    expect(more().textContent).toBe('Show all')
    // The clamp ends in the UI face's ellipsis, three dots wide, never a
    // mono cell's ellipsis that reads as a period.
    await document.fonts.ready
    const style = getComputedStyle(line)
    expect(style.fontFamily.split(',')[0].replaceAll(/['"]/g, '').trim()).toBe('Schibsted Grotesk')
    const pen = document.createElement('canvas').getContext('2d')
    pen.font = `${style.fontSize} ${style.fontFamily}`
    expect(pen.measureText('\u2026').width).toBeGreaterThan(2.5 * pen.measureText('.').width)
    await userEvent.click(more())
    expect(line.classList.contains('open')).toBe(true)
    expect(line.scrollHeight).toBeLessThanOrEqual(line.clientHeight + 1)
    expect(more().textContent).toBe('Show less')
    await userEvent.click(more())
    await expect.poll(() => more()?.textContent).toBe('Show all')
  })
}

it('drops Show all when the profile widens until the reason fits, and shows it again when it narrows', async () => {
  const aside = profile({ activity: { state: 'error', action: reason, since: null } })
  const column = aside.parentElement
  const more = () => aside.querySelector('.state-more')
  await expect.poll(() => !!more()).toBe(true)
  column.style.width = `${RAIL_OCCUPANTS.agent.max}px`
  column.style.gridTemplateColumns = `${RAIL_OCCUPANTS.agent.max}px`
  await expect.poll(() => !!more()).toBe(false)
  column.style.width = `${RAIL_OCCUPANTS.agent.min}px`
  column.style.gridTemplateColumns = `${RAIL_OCCUPANTS.agent.min}px`
  await expect.poll(() => !!more()).toBe(true)
})

it('scrolls the profile\'s own delete confirmation into view and focuses Cancel, which Space activates', async () => {
  const aside = profile({ width: 320, height: 420 })
  const body = aside.querySelector('.body')
  const remove = [...aside.querySelectorAll('button')].find((button) => button.textContent.trim() === 'Delete agent')
  // The button sits below the window bottom until the body scrolls to it.
  remove.scrollIntoView({ block: 'end' })
  await userEvent.click(remove)
  const group = () => aside.querySelector('[role="group"][aria-label="Delete agent"]')
  await expect.poll(group).toBeTruthy()
  const confirm = group()
  const cancel = [...confirm.querySelectorAll('button')].find((button) => button.textContent.trim() === 'Cancel')
  await expect.poll(() => document.activeElement).toBe(cancel)
  const view = body.getBoundingClientRect()
  const box = confirm.getBoundingClientRect()
  expect(box.top).toBeGreaterThanOrEqual(view.top)
  expect(box.bottom).toBeLessThanOrEqual(view.bottom)
  await userEvent.keyboard(' ')
  await expect.poll(group).toBeNull()
})

it('returns focus to Delete agent after Cancel, by pointer or by key, on the profile and the card path', async () => {
  const remove = (aside) => [...aside.querySelectorAll('button')].find((button) => button.textContent.trim() === 'Delete agent')
  const cancel = (aside) => [...aside.querySelectorAll('[role="group"][aria-label="Delete agent"] button')].find((button) => button.textContent.trim() === 'Cancel')
  // The profile's own button opens the confirmation, and a click on Cancel closes it.
  let aside = profile()
  await userEvent.click(remove(aside))
  await expect.poll(() => document.activeElement).toBe(cancel(aside))
  await userEvent.click(cancel(aside))
  await expect.poll(() => document.activeElement).toBe(remove(aside))
  unmount(component)
  document.body.replaceChildren()
  // A card's Delete opens the profile at the confirmation, and Enter on Cancel closes it.
  aside = profile({ deleteAsked: 1 })
  await expect.poll(() => document.activeElement).toBe(cancel(aside))
  await userEvent.keyboard('{Enter}')
  await expect.poll(() => document.activeElement).toBe(remove(aside))
  expect(aside.querySelector('[role="group"][aria-label="Delete agent"]')).toBeNull()
})

const ringed = (node) => getComputedStyle(node).outlineStyle === 'solid' && getComputedStyle(node).outlineWidth === '1px'

it('shows the focus ring where code moves focus after the keys, on Cancel and back on Delete agent, and none after a click', async () => {
  const remove = (aside) => [...aside.querySelectorAll('button')].find((button) => button.textContent.trim() === 'Delete agent')
  const cancel = (aside) => [...aside.querySelectorAll('[role="group"][aria-label="Delete agent"] button')].find((button) => button.textContent.trim() === 'Cancel')
  const stop = trackFocusSource(document.documentElement)
  try {
    // The profile's own button, pressed by key: Cancel takes focus with the ring, and Cancel by key returns it, ring and all.
    let aside = profile()
    remove(aside).focus()
    await userEvent.keyboard('{Enter}')
    await expect.poll(() => document.activeElement).toBe(cancel(aside))
    expect(ringed(cancel(aside))).toBe(true)
    await userEvent.keyboard('{Enter}')
    await expect.poll(() => document.activeElement).toBe(remove(aside))
    expect(ringed(remove(aside))).toBe(true)
    // Escape cancels too, and focus returns with the ring.
    await userEvent.keyboard('{Enter}')
    await expect.poll(() => document.activeElement).toBe(cancel(aside))
    await userEvent.keyboard('{Escape}')
    await expect.poll(() => document.activeElement).toBe(remove(aside))
    expect(aside.querySelector('[role="group"][aria-label="Delete agent"]')).toBeNull()
    expect(ringed(remove(aside))).toBe(true)
    // A click draws no ring where focus lands.
    await userEvent.click(remove(aside))
    await expect.poll(() => document.activeElement).toBe(cancel(aside))
    expect(ringed(cancel(aside))).toBe(false)
    await userEvent.click(cancel(aside))
    await expect.poll(() => document.activeElement).toBe(remove(aside))
    expect(ringed(remove(aside))).toBe(false)
    // A click opens it and Enter on Cancel closes it: the key leads, so focus returns with the ring.
    await userEvent.click(remove(aside))
    await expect.poll(() => document.activeElement).toBe(cancel(aside))
    expect(ringed(cancel(aside))).toBe(false)
    await userEvent.keyboard('{Enter}')
    await expect.poll(() => document.activeElement).toBe(remove(aside))
    expect(ringed(remove(aside))).toBe(true)
    unmount(component)
    document.body.replaceChildren()
    // A card's Delete chosen by key opens the profile at the confirmation, and Cancel shows the ring.
    delete document.documentElement.dataset.pointer
    aside = profile({ deleteAsked: 1 })
    await expect.poll(() => document.activeElement).toBe(cancel(aside))
    expect(ringed(cancel(aside))).toBe(true)
  } finally { stop(); delete document.documentElement.dataset.pointer }
})

it('draws the ring on a control code focused while the keys lead, when the engine matches no :focus-visible', async () => {
  const button = document.createElement('button')
  button.textContent = 'Delete agent'
  document.body.append(button)
  button.setAttribute('data-key-focus', '')
  button.focus({ focusVisible: false })
  expect(ringed(button)).toBe(true)
  document.documentElement.dataset.pointer = ''
  try { expect(ringed(button)).toBe(false) } finally { delete document.documentElement.dataset.pointer }
})

import { afterEach, expect, it } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import AgentCard from './AgentCard.svelte'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

const agent = { id: 'scout', name: 'Scout', label: 'Researcher', runtimeId: 'local', projectId: null, routines: [] }
const tauri = { invoke: async (command, args) => args?.operation === 'agent.runs' ? { runs: [], next_cursor: null } : {} }

// A card as the Agents dashboard lays it out, `left` pixels from the window's left edge.
function card(left) {
  const target = document.createElement('div')
  Object.assign(target.style, { position: 'absolute', left: `${left}px`, top: '40px', width: '340px', height: '300px' })
  document.body.append(target)
  component = mount(AgentCard, { target, props: { agent, tauri, machine: { id: 'local', name: 'This computer', kind: 'laptop' } } })
  flushSync()
  return target
}

const overlaps = (a, b) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom

// The gap from a rail mark's icon to its name, and the vertical offset of
// their centers.
function nearIcon(mark, box) {
  const icon = mark.querySelector('svg').getBoundingClientRect()
  const gap = Math.max(box.left - icon.right, icon.left - box.right, 0)
  const offset = Math.abs((box.top + box.bottom) / 2 - (icon.top + icon.bottom) / 2)
  return { gap, offset }
}

it('names a rail mark right of its icon, centered on it, clear of the sidebar at its left', async () => {
  const sidebar = document.createElement('nav')
  Object.assign(sidebar.style, { position: 'absolute', left: '0', top: '0', width: '150px', height: '400px' })
  document.body.append(sidebar)
  await page.viewport(1000, 600)
  const target = card(160)
  for (const name of ['Overview', 'History']) {
    const mark = target.querySelector(`[role="tab"][aria-label="${name}"]`)
    await userEvent.hover(mark)
    const tip = document.querySelector('.rail-tip')
    expect(tip.textContent).toBe(name)
    const box = tip.getBoundingClientRect()
    expect(box.left).toBeGreaterThanOrEqual(mark.getBoundingClientRect().right)
    const { gap, offset } = nearIcon(mark, box)
    expect(gap).toBeLessThan(16)
    expect(offset).toBeLessThanOrEqual(1)
    expect(box.right).toBeLessThanOrEqual(window.innerWidth)
    expect(overlaps(box, sidebar.getBoundingClientRect())).toBe(false)
  }
})

it('names a rail mark left of its icon only where the window\'s right edge cuts it', async () => {
  await page.viewport(1000, 600)
  // A card pushed past the window's right edge, so its rail sits at that edge.
  const target = card(window.innerWidth - 60)
  const mark = target.querySelector('[role="tab"][aria-label="Permissions"]')
  await userEvent.hover(mark, { force: true })
  const box = document.querySelector('.rail-tip').getBoundingClientRect()
  expect(box.right).toBeLessThanOrEqual(mark.getBoundingClientRect().left)
  expect(box.left).toBeGreaterThanOrEqual(0)
  const { gap, offset } = nearIcon(mark, box)
  expect(gap).toBeLessThan(16)
  expect(offset).toBeLessThanOrEqual(1)
})

// The picked mark's edge spans the mark and meets the rail's divider, where a
// short bar beside the icon read as a text caret.
it('draws the picked mark edge the full mark height against the rail divider', () => {
  const rail = card(20).querySelector('.panel-rail')
  const mark = rail.querySelector('[aria-selected="true"]')
  const edge = getComputedStyle(mark, '::after')
  expect(edge.height).toBe(`${mark.clientHeight}px`)
  const edgeRight = mark.getBoundingClientRect().right - parseFloat(getComputedStyle(mark).borderRightWidth) - parseFloat(edge.right)
  expect(edgeRight).toBe(rail.getBoundingClientRect().right - parseFloat(getComputedStyle(rail).borderRightWidth))
})

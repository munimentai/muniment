import { afterEach, expect, it } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import AgentCard from './AgentCard.svelte'

let component
const components = []
afterEach(() => {
  if (component) unmount(component)
  component = null
  components.splice(0).forEach((mounted) => unmount(mounted))
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

// The gap from a rail mark's icon to its name, in any direction.
function iconGap(mark, box) {
  const icon = mark.querySelector('svg').getBoundingClientRect()
  return Math.hypot(Math.max(box.left - icon.right, icon.left - box.right, 0), Math.max(box.top - icon.bottom, icon.top - box.bottom, 0))
}

// What a panel shows: its heading and every word and icon of its rows.
function panelContent(target) {
  const panel = target.querySelector('[role="tabpanel"]')
  const boxes = [...panel.querySelectorAll('h4, svg')].map((node) => node.getBoundingClientRect())
  const walker = document.createTreeWalker(panel, NodeFilter.SHOW_TEXT)
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.textContent.trim()) continue
    const range = document.createRange()
    range.selectNodeContents(node)
    boxes.push(...range.getClientRects())
  }
  return boxes.filter((box) => box.width && box.height)
}

// The Agents page as the app lays it out: the sidebar, then the page with its
// 24px margin and a grid of 340 by 300 cards 12px apart.
function agentsPage(count = 2) {
  const sidebar = document.createElement('nav')
  sidebar.id = 'sidebar'
  Object.assign(sidebar.style, { position: 'absolute', left: '0', top: '0', width: '240px', height: '100%' })
  const page = document.createElement('main')
  Object.assign(page.style, { position: 'absolute', left: '240px', top: '0', right: '0', padding: '24px', display: 'grid', gridTemplateColumns: 'repeat(auto-fill, 340px)', gridAutoRows: '300px', gap: '12px' })
  document.body.append(sidebar, page)
  const cards = Array.from({ length: count }, (_, index) => {
    const target = document.createElement('div')
    page.append(target)
    components.push(mount(AgentCard, { target, props: { agent: { ...agent, id: `scout-${index}`, name: `Scout ${index}` }, tauri, machine: { id: 'local', name: 'This computer', kind: 'laptop' } } }))
    return target
  })
  flushSync()
  return { sidebar, cards }
}

it('names each rail mark next to its icon, clear of the panel heading, its rows and the sidebar', async () => {
  await page.viewport(1100, 600)
  const { sidebar, cards } = agentsPage(2)
  for (const target of cards) {
    // Overview holds the heading and the fact rows, and Routines the "Add or edit in the profile" row.
    for (const shown of ['Overview', 'Routines']) {
      await userEvent.click(target.querySelector(`[role="tab"][aria-label="${shown}"]`))
      for (const name of ['Overview', 'Model', 'Permissions', 'Routines', 'History']) {
        const mark = target.querySelector(`[role="tab"][aria-label="${name}"]`)
        await userEvent.hover(mark)
        const tip = document.querySelector('.rail-tip')
        expect(tip.textContent).toBe(name)
        const box = tip.getBoundingClientRect()
        expect(iconGap(mark, box)).toBeLessThan(16)
        expect(overlaps(box, mark.querySelector('svg').getBoundingClientRect())).toBe(false)
        expect(overlaps(box, sidebar.getBoundingClientRect())).toBe(false)
        expect(panelContent(target).filter((content) => overlaps(box, content)).map((content) => `${name}/${shown} ${JSON.stringify(box)} ${JSON.stringify(content)}`)).toEqual([])
        expect(box.left).toBeGreaterThanOrEqual(0)
        expect(box.top).toBeGreaterThanOrEqual(0)
      }
    }
  }
  expect(cards[0].querySelector('h4').textContent).toBe('Routines')
  expect(cards[0].querySelector('.foot').textContent).toBe('Add or edit in the profile')
})

it('names a rail mark left of its icon, outside the card, where the page is free there', async () => {
  await page.viewport(1100, 600)
  const sidebar = document.createElement('nav')
  sidebar.id = 'sidebar'
  Object.assign(sidebar.style, { position: 'absolute', left: '0', top: '0', width: '150px', height: '400px' })
  document.body.append(sidebar)
  const target = card(300)
  const mark = target.querySelector('[role="tab"][aria-label="Permissions"]')
  await userEvent.hover(mark)
  const box = document.querySelector('.rail-tip').getBoundingClientRect()
  const icon = mark.querySelector('svg').getBoundingClientRect()
  expect(box.right).toBeLessThanOrEqual(target.getBoundingClientRect().left + 16)
  expect(box.right).toBeLessThanOrEqual(icon.left)
  expect(Math.abs((box.top + box.bottom) / 2 - (icon.top + icon.bottom) / 2)).toBeLessThanOrEqual(1)
  expect(iconGap(mark, box)).toBeLessThan(16)
  expect(overlaps(box, sidebar.getBoundingClientRect())).toBe(false)
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

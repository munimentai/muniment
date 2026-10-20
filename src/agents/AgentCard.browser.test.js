import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
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

it('names a rail mark beside the card, clear of the panel labels', async () => {
  const target = card(160)
  await userEvent.hover(target.querySelector('[role="tab"][aria-label="Model"]'))
  const tip = document.querySelector('.rail-tip')
  expect(tip.textContent).toBe('Model')
  const box = tip.getBoundingClientRect()
  expect(box.right).toBeLessThanOrEqual(target.querySelector('.agent-card').getBoundingClientRect().left)
  for (const label of target.querySelectorAll('dt')) expect(overlaps(box, label.getBoundingClientRect())).toBe(false)
})

it('names a rail mark beside the right edge of a card at the window edge', async () => {
  const target = card(0)
  await userEvent.hover(target.querySelector('[role="tab"][aria-label="Permissions"]'))
  const box = document.querySelector('.rail-tip').getBoundingClientRect()
  expect(box.left).toBeGreaterThanOrEqual(target.querySelector('.agent-card').getBoundingClientRect().right)
  for (const label of target.querySelectorAll('dt')) expect(overlaps(box, label.getBoundingClientRect())).toBe(false)
})

import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import UsagePage from './UsagePage.svelte'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

const runtimeList = { runtimes: [
  { id: 'local', name: 'This computer', kind: 'laptop', local: true, status: 'online' },
  { id: 'pod', name: 'Docker container', kind: 'container', status: 'offline' },
] }
const summary = { range: '7d', bucket: 'day', totals: { processed: 0, runs: 0, cost_usd: 0 }, series: [], breakdown: {}, providers: [], accounts: [], unmetered: [], machines: [] }

it('keeps a long machine name on one line with an ellipsis, the whole name on hover, and the header still', async () => {
  const target = document.createElement('div')
  Object.assign(target.style, { position: 'absolute', inset: '0', display: 'grid', gridTemplateAreas: '"thread"' })
  document.body.append(target)
  component = mount(UsagePage, { target, props: { tauri: { invoke: async () => summary }, runtimeList } })
  flushSync()
  const trigger = () => target.querySelector('button[aria-label^="Machine:"]')
  const head = target.querySelector('.usage-head')
  // The header's height and the title's place in it, which a wrapped filter changes.
  const header = () => ({ height: head.getBoundingClientRect().height, title: target.querySelector('h2').getBoundingClientRect().top - head.getBoundingClientRect().top })
  const filter = trigger().closest('.filter')
  await expect.poll(() => target.textContent.includes('Reading usage')).toBe(false)
  const before = { trigger: trigger().getBoundingClientRect().height, header: header() }
  await userEvent.click(trigger())
  await userEvent.click([...target.querySelectorAll('[role="menuitemradio"]')].find((item) => item.textContent.trim() === 'Docker container (offline)'))
  await expect.poll(() => trigger().getAttribute('aria-label')).toBe('Machine: Docker container (offline)')
  const label = trigger().querySelector('span')
  expect(getComputedStyle(label).whiteSpace).toBe('nowrap')
  expect(label.scrollWidth).toBeGreaterThan(label.clientWidth)
  expect(getComputedStyle(label).textOverflow).toBe('ellipsis')
  expect(trigger().getBoundingClientRect().height).toBe(before.trigger)
  // A scrolled page snaps the edges to the half pixel, and a wrapped name moves them by a line.
  expect(header().height).toBe(before.header.height)
  expect(Math.abs(header().title - before.header.title)).toBeLessThan(1)
  // Hover names the whole machine in the app's tooltip.
  await userEvent.hover(trigger())
  await expect.poll(() => filter.dataset.tooltip).toBe('Docker container (offline)')
  await expect.poll(() => getComputedStyle(filter, '::after').opacity).toBe('1')
  // A name that fits needs no tooltip.
  await userEvent.click(trigger())
  await userEvent.click([...target.querySelectorAll('[role="menuitemradio"]')].find((item) => item.textContent.trim() === 'All machines'))
  await userEvent.unhover(trigger())
  await userEvent.hover(trigger())
  await expect.poll(() => filter.dataset.tooltip).toBeUndefined()
})

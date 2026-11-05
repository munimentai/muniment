import { afterEach, expect, it } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { cleanup, render } from '@testing-library/svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import ModelPicker from './ModelPicker.svelte'
afterEach(cleanup)

it('shows search focus once, on the search row, and not as a second ring on the field', async () => {
  const { container } = render(ModelPicker, { inventory: { providers: [{ id: 'openai', name: 'OpenAI', source: 'key', models: [{ id: 'gpt-6.1-sol' }] }] }, onchoose() {}, onmanage() {}, onclose() {} })
  const field = container.querySelector('input[type="search"]')
  const row = container.querySelector('.picker-search')
  const resting = getComputedStyle(row).borderBottomColor
  field.focus()
  await userEvent.keyboard('g')
  expect(document.activeElement).toBe(field)
  expect(field.matches(':focus-visible')).toBe(true)
  expect(getComputedStyle(field).outlineStyle).toBe('none')
  expect(getComputedStyle(row).outlineStyle).toBe('none')
  expect(getComputedStyle(row).borderBottomColor).not.toBe(resting)
})

const overlaps = (a, b) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom

// Every word, icon and heading the list shows.
function listContent(picker) {
  const list = picker.querySelector('.picker-list')
  const boxes = [...list.querySelectorAll('h4, svg')].map((node) => node.getBoundingClientRect())
  const walker = document.createTreeWalker(list, NodeFilter.SHOW_TEXT)
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.textContent.trim()) continue
    const range = document.createRange()
    range.selectNodeContents(node)
    boxes.push(...range.getClientRects())
  }
  return boxes.filter((box) => box.width && box.height)
}

const railInventory = { providers: [
  { id: 'xai', name: 'xAI', source: 'key', models: [{ id: 'grok-4.20-0309-reasoning' }, { id: 'grok-4.20-0309-non-reasoning' }, { id: 'grok-4.20-multi-agent-0309' }] },
  { id: 'anthropic', name: 'Anthropic', source: 'key', models: [{ id: 'claude-sonnet-4-5' }, { id: 'claude-opus-5-5' }] },
  { id: 'openai', name: 'OpenAI', source: 'key', models: [{ id: 'gpt-6.1-sol' }] },
] }

// The sidebar, and the picker at `left` beside it, as the composer or an agent card holds it.
function pickerAt(left, embedded = false) {
  const sidebar = document.createElement('nav')
  sidebar.id = 'sidebar'
  Object.assign(sidebar.style, { position: 'absolute', left: '0', top: '0', width: '240px', height: '100%' })
  const host = document.createElement('div')
  // The composer's picker opens above the composer, and an agent card's fills its panel.
  Object.assign(host.style, embedded
    ? { position: 'absolute', left: `${left}px`, top: '120px', width: '300px', height: '360px', display: 'flex' }
    : { position: 'absolute', left: `${left}px`, top: '560px', width: '420px', height: '40px' })
  document.body.append(sidebar, host)
  render(ModelPicker, { target: host, props: { inventory: railInventory, embedded, current: { provider: 'xai', model: 'grok-4.20-0309-reasoning' }, onchoose() {}, onmanage() {}, onclose() {} } })
  return { sidebar, picker: host.querySelector('[data-panel="model-picker"]') }
}

// The logo the mark shows. A logo with a light and a dark variant keeps the
// other theme's copy hidden in the mark.
const shownIcon = (mark) => [...mark.querySelectorAll('svg')].map((node) => node.getBoundingClientRect()).find((box) => box.width && box.height)

afterEach(() => { delete document.documentElement.dataset.scheme })

for (const scheme of ['light', 'dark']) for (const [where, left, embedded] of [['with free page at its left', 400, false], ['beside the sidebar', 240, false], ['embedded in an agent card', 300, true]]) {
  it(`names each rail mark next to its icon and never over a model name, ${where}, in the ${scheme} scheme`, async () => {
    document.documentElement.dataset.scheme = scheme
    await page.viewport(1100, 700)
    const { sidebar, picker } = pickerAt(left, embedded)
    expect(listContent(picker).length).toBeGreaterThan(0)
    for (const name of ['xAI', 'Anthropic', 'OpenAI']) {
      const mark = picker.querySelector(`.rail-mark[aria-label="${name}"]`)
      await userEvent.hover(mark)
      const tip = document.querySelector('.rail-tip')
      expect(tip.textContent).toBe(name)
      const box = tip.getBoundingClientRect()
      const icon = shownIcon(mark)
      expect(icon).toBeTruthy()
      const gap = Math.hypot(Math.max(box.left - icon.right, icon.left - box.right, 0), Math.max(box.top - icon.bottom, icon.top - box.bottom, 0))
      expect(gap).toBeLessThan(24)
      expect(overlaps(box, icon)).toBe(false)
      // Only a name longer than the rail and the gutter beside the sidebar reaches the sidebar's edge.
      if (name === 'xAI' || where !== 'beside the sidebar') expect(overlaps(box, sidebar.getBoundingClientRect())).toBe(false)
      expect(listContent(picker).filter((content) => overlaps(box, content)).map((content) => `${name} ${JSON.stringify(box)} ${JSON.stringify(content)}`)).toEqual([])
      if (where === 'with free page at its left') expect(box.right).toBeLessThanOrEqual(picker.getBoundingClientRect().left)
    }
    expect(picker.textContent).toContain('Grok 4.20 0309 Reasoning')
  })
}

it('selects no word of a row label, while the search field keeps its text selectable', () => {
  const { container } = render(ModelPicker, { inventory: { providers: [{ id: 'openai', name: 'OpenAI', source: 'key', models: [{ id: 'astra-large' }] }] }, onchoose() {}, onmanage() {}, onclose() {} })
  const row = container.querySelector('.picker-row')
  for (const node of [row, row.querySelector('.model-id'), container.querySelector('.rail-mark')]) expect(getComputedStyle(node).userSelect).toBe('none')
  expect(getComputedStyle(container.querySelector('input[type="search"]')).userSelect).not.toBe('none')
})

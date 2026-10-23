import { afterEach, expect, it } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import LimitCard from './LimitCard.svelte'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

const NOW = Date.UTC(2026, 9, 17, 12)
const HOUR = 3_600_000
const row = (kind, scope, remaining, extra = {}) => ({ provider: 'anthropic', kind, scope, remaining_percent: remaining, resets_at_ms: NOW + 3 * HOUR, status: 'ok', ...extra })
const account = {
  provider: 'anthropic', identity: 'anthropic:email:someone@example.com:', account_id: 'a', name: 'Anthropic',
  subtitle: 'someone.with.a.long.address@example.com · max',
  windows: [row('five_hour', '', 62), row('weekly', '', 80), row('weekly', 'Fable', 12, { status: 'low' }), row('weekly', 'Long model name', 0, { status: 'limit' })],
}
const unmetered = { provider: 'omlx', label: 'A local server with a long name', reason: 'local', endpoint: '127.0.0.1:18080', machines: ['local'], usage: { input: 842_000, output: 398_000, processed: 1_240_000, runs: 37 } }

function card(width, props = { account, now: NOW }) {
  const target = document.createElement('div')
  target.style.width = `${width}px`
  document.body.append(target)
  component = mount(LimitCard, { target, props })
  flushSync()
  return target.querySelector('.limit-card')
}

function inside(article, selector) {
  const box = article.getBoundingClientRect()
  for (const part of article.querySelectorAll(selector)) {
    const rect = part.getBoundingClientRect()
    expect(rect.left).toBeGreaterThanOrEqual(box.left)
    expect(rect.right).toBeLessThanOrEqual(box.right)
  }
}

it('keeps every row on one line inside a narrow card', () => {
  const article = card(200)
  expect(article.scrollWidth).toBeLessThanOrEqual(article.clientWidth)
  inside(article, '.meter, .left, .account')
  const rows = [...article.querySelectorAll('.window')]
  expect(rows).toHaveLength(4)
  // Name, meter and share left sit side by side in one row.
  for (const item of rows) {
    const [name, meter, left] = ['.window-name', '.meter', '.left'].map((selector) => item.querySelector(selector).getBoundingClientRect())
    expect(name.right).toBeLessThanOrEqual(meter.left)
    expect(meter.right).toBeLessThanOrEqual(left.left)
  }
})

it('lights the meter steps in the fixed meter colors', () => {
  const article = card(320)
  const lit = (meter) => [...meter.querySelectorAll('i')].map((step) => getComputedStyle(step).backgroundColor)
  const [high, full, low, spent] = [...article.querySelectorAll('.meter')].map(lit)
  const off = getComputedStyle(document.documentElement).getPropertyValue('--meter-off').trim()
  expect(off).not.toBe('')
  expect(new Set(full).size).toBe(1)
  expect(high.slice(0, 3)).toEqual([high[0], high[0], high[0]])
  expect(high[3]).not.toBe(high[0])
  expect(low[0]).not.toBe(low[1])
  expect(new Set(spent).size).toBe(1)
  expect(spent[0]).toBe(low[1])
})

it('keeps a provider with no limit inside a narrow card', () => {
  const article = card(200, { unmetered, range: '7d', machines: [] })
  expect(article.scrollWidth).toBeLessThanOrEqual(article.clientWidth)
  inside(article, '.slot, .account, .tokens b, .note')
})

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
const row = (kind, scope, remaining, extra = {}) => ({ provider: 'anthropic', kind, scope, remaining_percent: remaining, resets_at_ms: NOW + 3 * HOUR, elapsed_percent: 40, pace: 'on', status: 'ok', ...extra })
const account = {
  provider: 'anthropic', identity: 'anthropic:email:someone@example.com:', account_id: 'a', name: 'Anthropic',
  subtitle: 'someone.with.a.long.address@example.com · max', pace: 'ahead',
  windows: [row('five_hour', '', 62), row('weekly', '', 80), row('weekly', 'Fable', 12, { status: 'low' })],
}

function card(width) {
  const target = document.createElement('div')
  target.style.width = `${width}px`
  document.body.append(target)
  component = mount(LimitCard, { target, props: { account, now: NOW } })
  flushSync()
  return target.querySelector('.limit-card')
}

it('keeps every row inside a narrow card', () => {
  const article = card(200)
  expect(article.scrollWidth).toBeLessThanOrEqual(article.clientWidth)
  const box = article.getBoundingClientRect()
  for (const bar of article.querySelectorAll('.bar')) {
    const rect = bar.getBoundingClientRect()
    expect(rect.left).toBeGreaterThanOrEqual(box.left)
    expect(rect.right).toBeLessThanOrEqual(box.right)
  }
  const subtitle = article.querySelector('.account')
  expect(subtitle.getBoundingClientRect().right).toBeLessThanOrEqual(box.right)
  expect(article.querySelectorAll('.window')).toHaveLength(3)
})

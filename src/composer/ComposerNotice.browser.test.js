import { afterEach, expect, it } from 'vitest'
import { createRawSnippet, flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import ChatComposer from './ChatComposer.svelte'
import ComposerNotice from './ComposerNotice.svelte'
import ModePicker from './ModePicker.svelte'
import { createComposerNotices } from '../lib/composer-notices.js'

let components = []
afterEach(() => {
  delete document.documentElement.dataset.motion
  for (const component of components.reverse()) unmount(component)
  components = []
  document.body.replaceChildren()
})

const field = createRawSnippet(() => ({ render: () => '<textarea rows="2"></textarea>' }))
const modes = [{ id: 'ask', title: 'Ask before changes', description: 'Asks before each change.' }]

// The composer near the foot of the window with a notice showing over it.
function composerWithNotice() {
  const host = document.createElement('div')
  host.style.cssText = 'padding-top: 400px;'
  document.body.append(host)
  components.push(mount(ChatComposer, { target: host, props: { children: field } }))
  const composer = host.querySelector('.composer')
  const notices = createComposerNotices()
  components.push(mount(ComposerNotice, { target: composer, props: { notices } }))
  notices.show({ id: 'one', text: 'Scheduled agents run only while Muniment is open.', action: { label: 'Open Login Items', run: () => {} } })
  flushSync()
  return { composer, notice: composer.querySelector('[data-composer-notice]') }
}

it('sits above the composer at the composer menus anchor, on their surface, in the UI face', () => {
  // Reduced motion settles the entrance at once, so the box rests in place.
  document.documentElement.dataset.motion = 'reduce'
  const { composer, notice } = composerWithNotice()
  const frame = composer.getBoundingClientRect()
  const box = notice.getBoundingClientRect()
  expect(notice.checkVisibility()).toBe(true)
  expect(getComputedStyle(notice).animationDuration).toBe('1e-06s')
  expect(Math.round(frame.top + composer.clientTop - box.bottom)).toBe(8)
  expect(Math.round(box.left)).toBe(Math.round(frame.left + composer.clientLeft))
  expect(box.width).toBeLessThanOrEqual(frame.width)

  components.push(mount(ModePicker, { target: composer, props: { modes, current: 'ask', onchoose: () => {}, onclose: () => {} } }))
  flushSync()
  const menu = composer.querySelector('[data-panel="mode-picker"]')
  const menuBox = menu.getBoundingClientRect()
  expect(Math.round(menuBox.bottom)).toBe(Math.round(box.bottom))
  expect(Math.round(menuBox.left)).toBe(Math.round(box.left))
  const [noticeStyle, menuStyle] = [getComputedStyle(notice), getComputedStyle(menu)]
  for (const property of ['background-color', 'border-top-color', 'border-top-left-radius', 'box-shadow']) {
    expect(noticeStyle.getPropertyValue(property)).toBe(menuStyle.getPropertyValue(property))
  }
  const text = getComputedStyle(notice.querySelector('p')).fontFamily
  expect(text).toBe(getComputedStyle(menu.querySelector('.mode-title')).fontFamily)
  expect(text).not.toMatch(/mono/i)
})

it('hides under an open composer menu and returns once it closes', () => {
  const { composer, notice } = composerWithNotice()
  const picker = mount(ModePicker, { target: composer, props: { modes, current: 'ask', onchoose: () => {}, onclose: () => {} } })
  flushSync()
  expect(notice.checkVisibility({ visibilityProperty: true })).toBe(false)
  unmount(picker)
  flushSync()
  expect(notice.checkVisibility({ visibilityProperty: true })).toBe(true)
})

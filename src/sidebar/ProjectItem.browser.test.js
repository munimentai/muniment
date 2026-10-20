import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { flushSync, mount, unmount } from 'svelte'
import { trackFocusSource } from '../lib/focus-source.js'
import '../styles/tokens.css'
import '../styles/base.css'
import ProjectItem from './ProjectItem.svelte'

let component
afterEach(() => {
  delete document.documentElement.dataset.pointer
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

// A sidebar row as wide as the default sidebar, or `width` wide.
function row(props, width = 184) {
  const target = document.createElement('div')
  target.style.width = `${width}px`
  document.body.append(target)
  component = mount(ProjectItem, { target, props: { expanded: false, ontoggle() {}, onnew() {}, ...props } })
  flushSync()
  return target
}

const clipped = (element) => element.scrollWidth - element.clientWidth > 1

it('shows the triggers in full and clips the project name first', () => {
  const target = row({ name: 'Tauri Morning Briefing', triggers: 'weekdays 8:00 +1' })
  const name = target.querySelector('.project-title > span')
  const triggers = target.querySelector('.triggers')
  expect(clipped(triggers)).toBe(false)
  expect(clipped(name)).toBe(true)
})

it('keeps a short name and its triggers whole', () => {
  const target = row({ name: 'News', triggers: 'every day 9:00' })
  expect(clipped(target.querySelector('.project-title > span'))).toBe(false)
  expect(clipped(target.querySelector('.triggers'))).toBe(false)
})

// Whether `element` shows on the first line of the triggers, not wrapped onto the hidden second one.
const onFirstLine = (element) => element.getBoundingClientRect().top < element.parentElement.getBoundingClientRect().bottom

// Hovers the row and waits for its controls to show.
async function hovered(target) {
  await userEvent.hover(target.querySelector('.project-title'))
  const controls = target.querySelector('.project-controls')
  await expect.poll(() => getComputedStyle(controls).opacity).toBe('1')
  return controls.getBoundingClientRect().left
}

it('keeps the whole triggers clear of the controls on a wide hovered row', async () => {
  const target = row({ name: 'Tauri Morning Briefing', triggers: 'weekdays 8:00 +1', onsettings() {} }, 260)
  const controls = await hovered(target)
  const triggers = target.querySelector('.triggers')
  expect(clipped(triggers)).toBe(false)
  expect(onFirstLine(target.querySelector('.trigger-more'))).toBe(true)
  expect(clipped(target.querySelector('.project-title > span'))).toBe(true)
  expect(triggers.getBoundingClientRect().right).toBeLessThanOrEqual(controls)
})

it('drops the extra trigger count before the time when the hovered row runs short', async () => {
  const target = row({ name: 'Tauri Morning Briefing', triggers: 'weekdays 8:00 +1', onsettings() {} })
  const controls = await hovered(target)
  const words = target.querySelector('.trigger-words')
  expect(words.textContent).toBe('weekdays 8:00')
  expect(clipped(words)).toBe(false)
  expect(onFirstLine(target.querySelector('.trigger-more'))).toBe(false)
  expect(clipped(target.querySelector('.project-title > span'))).toBe(true)
  expect(words.getBoundingClientRect().right).toBeLessThanOrEqual(controls)
})

// The menu items that show a fill, by name.
const filled = (target) => [...target.querySelectorAll('[role="menuitem"]')]
  .filter((item) => getComputedStyle(item).backgroundColor !== 'rgba(0, 0, 0, 0)')
  .map((item) => item.textContent.trim())

it('fills only the hovered menu item after the keys open the menu', async () => {
  const stop = trackFocusSource(document.documentElement)
  try {
    const target = row({ name: 'Research', onsettings() {}, onrename() {} })
    const trigger = target.querySelector('[aria-haspopup="menu"]')
    trigger.focus()
    await userEvent.keyboard('{Enter}')
    await expect.poll(() => document.activeElement?.textContent.trim()).toBe('New thread')
    expect(filled(target)).toEqual(['New thread'])
    await userEvent.hover(target.querySelector('[role="menu"]').children[2])
    expect(document.activeElement.textContent.trim()).toBe('Rename')
    expect(filled(target)).toEqual(['Rename'])
    await userEvent.keyboard('{ArrowDown}')
    expect(document.activeElement.textContent.trim()).toBe('New thread')
    expect(filled(target)).toEqual(['New thread'])
  } finally { stop() }
})

it('fills only the hovered menu item after a click opens the menu', async () => {
  const stop = trackFocusSource(document.documentElement)
  try {
    const target = row({ name: 'Research', onsettings() {}, onrename() {} })
    await userEvent.hover(target.querySelector('.project-title'))
    await userEvent.click(target.querySelector('[aria-haspopup="menu"]'))
    await expect.poll(() => document.activeElement?.textContent.trim()).toBe('New thread')
    expect(filled(target)).toEqual([])
    await userEvent.hover(target.querySelector('[role="menu"]').children[1])
    expect(filled(target)).toEqual(['Settings'])
  } finally { stop() }
})

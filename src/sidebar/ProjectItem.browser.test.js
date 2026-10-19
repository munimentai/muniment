import { afterEach, expect, it } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import ProjectItem from './ProjectItem.svelte'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

// A sidebar row as wide as the default sidebar.
function row(props) {
  const target = document.createElement('div')
  target.style.width = '184px'
  document.body.append(target)
  component = mount(ProjectItem, { target, props: { expanded: false, ontoggle() {}, onnew() {}, ...props } })
  flushSync()
  return target
}

const clipped = (element) => element.scrollWidth - element.clientWidth > 1

it('shows the triggers in full and clips the agent name first', () => {
  const target = row({ name: 'Tauri Morning Briefing', kind: 'agent', triggers: 'weekdays 8:00 +1' })
  const name = target.querySelector('.project-title > span')
  const triggers = target.querySelector('.triggers')
  expect(clipped(triggers)).toBe(false)
  expect(clipped(name)).toBe(true)
})

it('keeps a short name and its triggers whole', () => {
  const target = row({ name: 'News', kind: 'agent', triggers: 'every day 9:00' })
  expect(clipped(target.querySelector('.project-title > span'))).toBe(false)
  expect(clipped(target.querySelector('.triggers'))).toBe(false)
})

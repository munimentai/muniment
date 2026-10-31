import { afterEach, expect, it } from 'vitest'
import { landFocus, shown } from './focus-landing.js'

const SKIP = '#sidebar, .titlebar, [data-panel="workspace"]'
afterEach(() => { document.body.innerHTML = '' })

function page() {
  document.body.innerHTML = `
    <section class="workspace">
      <header class="titlebar"><h1>Pong test renamed</h1></header>
      <aside id="sidebar"><h2>Threads</h2></aside>
      <div class="thread"><textarea aria-label="Message"></textarea></div>
      <div class="settings-layout" hidden><h3>General</h3></div>
      <aside data-panel="workspace" class="workspace-panel"><h2>Terminal</h2><textarea aria-label="Terminal input"></textarea></aside>
    </section>`
  return document.querySelector('.workspace')
}

it('moves focus from the composer a page change hides to the new page heading', () => {
  const root = page()
  const composer = root.querySelector('[aria-label="Message"]')
  composer.focus()
  root.querySelector('.thread').style.visibility = 'hidden'
  root.querySelector('.settings-layout').hidden = false
  expect(landFocus(root, { skip: SKIP })).toBe(root.querySelector('.settings-layout h3'))
  expect(document.activeElement.textContent).toBe('General')
  expect(shown(document.activeElement)).toBe(true)
  // The heading takes focus from code only, never from Tab.
  expect(document.activeElement.getAttribute('tabindex')).toBe('-1')
})

it('moves focus from a hidden terminal to the composer when the page shows no heading', () => {
  const root = page()
  const composer = root.querySelector('[aria-label="Message"]')
  root.querySelector('[aria-label="Terminal input"]').focus()
  root.querySelector('.workspace-panel').style.display = 'none'
  expect(landFocus(root, { skip: SKIP, fallback: composer })).toBe(composer)
  expect(document.activeElement).toBe(composer)
})

it('leaves focus the user sees, and leaves a removed node for the body', () => {
  const root = page()
  const composer = root.querySelector('[aria-label="Message"]')
  composer.focus()
  expect(landFocus(root, { skip: SKIP })).toBeNull()
  expect(document.activeElement).toBe(composer)
  const terminal = root.querySelector('[aria-label="Terminal input"]')
  terminal.focus()
  terminal.remove()
  expect(shown(terminal)).toBe(false)
  landFocus(root, { skip: SKIP })
  expect(document.activeElement).toBe(document.body)
})

it('blurs a hidden node when the page offers no place to land', () => {
  const root = page()
  const composer = root.querySelector('[aria-label="Message"]')
  composer.focus()
  root.querySelector('.thread').style.visibility = 'hidden'
  expect(document.activeElement).toBe(composer)
  expect(landFocus(root, { skip: '.workspace' })).toBeNull()
  expect(document.activeElement).toBe(document.body)
})

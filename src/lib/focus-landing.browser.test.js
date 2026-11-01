import { afterEach, expect, it, vi } from 'vitest'
import { INPUT_MARK_CHECKS_MS, inputMarkSettler, landFocus, shown, takesText } from './focus-landing.js'

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

// WebKit gives a clicked sidebar button no focus and drops focus to the body,
// while the system's input-source mark stays at the composer's caret.
function catalog(heading) {
  const root = page()
  root.querySelector('.thread').insertAdjacentHTML('afterend', `<section aria-label="Agents" class="catalog" hidden>${heading ? '<h2>Agents</h2>' : ''}<p>Scout</p></section>`)
  const composer = root.querySelector('[aria-label="Message"]')
  composer.focus()
  composer.blur()
  root.querySelector('.thread').style.display = 'none'
  root.querySelector('.catalog').hidden = false
  expect(document.activeElement).toBe(document.body)
  return root
}

it('moves focus from the body to the heading of a page that replaced the thread', () => {
  const root = catalog(true)
  expect(landFocus(root, { skip: SKIP, page: true })).toBe(root.querySelector('.catalog h2'))
  expect(shown(document.activeElement)).toBe(true)
})

it('moves focus from the body to the region of a page with no heading', () => {
  const root = catalog(false)
  const region = root.querySelector('.catalog')
  expect(landFocus(root, { skip: SKIP, page: true })).toBe(region)
  expect(region.getAttribute('tabindex')).toBe('-1')
})

it('leaves focus on the body over a thread', () => {
  const root = page()
  expect(document.activeElement).toBe(document.body)
  expect(landFocus(root, { skip: SKIP })).toBeNull()
  expect(document.activeElement).toBe(document.body)
})

it('tells a field that takes typed text from a heading, a button and the body', () => {
  document.body.innerHTML = `
    <h2 tabindex="-1">Agents</h2><button>Run now</button><input type="checkbox" aria-label="Pick">
    <input aria-label="Name"><input type="search" aria-label="Search"><textarea aria-label="Message"></textarea>
    <div contenteditable="true">Draft</div>`
  const [heading, button, box, name, search, message, draft] = document.body.querySelectorAll('h2, button, input, textarea, div')
  expect([heading, button, box, document.body, null].map(takesText)).toEqual([false, false, false, false, false])
  expect([name, search, message, draft].map(takesText)).toEqual([true, true, true, true])
})

it('hides the input-source mark at one, two and three seconds after the last focus or keystroke', async () => {
  vi.useFakeTimers()
  try {
    const invoke = vi.fn(async () => {})
    const settler = inputMarkSettler(invoke)
    expect(INPUT_MARK_CHECKS_MS).toEqual([1000, 2000, 3000])
    settler.poke()
    vi.advanceTimersByTime(999)
    expect(invoke).not.toHaveBeenCalled()
    // Each keystroke starts the wait again.
    settler.poke()
    vi.advanceTimersByTime(600)
    settler.poke()
    vi.advanceTimersByTime(999)
    expect(invoke).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('input_mark_dismiss')
    // AppKit can show the mark after the first check, so two more follow.
    vi.advanceTimersByTime(999)
    expect(invoke).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1)
    expect(invoke).toHaveBeenCalledTimes(2)
    vi.advanceTimersByTime(1000)
    expect(invoke).toHaveBeenCalledTimes(3)
    vi.advanceTimersByTime(10000)
    expect(invoke).toHaveBeenCalledTimes(3)
    // A poke between checks starts the whole window again.
    settler.poke()
    vi.advanceTimersByTime(1500)
    expect(invoke).toHaveBeenCalledTimes(4)
    settler.poke()
    vi.advanceTimersByTime(2999)
    expect(invoke).toHaveBeenCalledTimes(6)
    vi.advanceTimersByTime(1)
    expect(invoke).toHaveBeenCalledTimes(7)
    vi.advanceTimersByTime(10000)
    expect(invoke).toHaveBeenCalledTimes(7)
    // A stopped settler asks nothing, also between checks, and a failed call stays quiet.
    settler.poke()
    settler.stop()
    vi.advanceTimersByTime(5000)
    expect(invoke).toHaveBeenCalledTimes(7)
    settler.poke()
    vi.advanceTimersByTime(1000)
    settler.stop()
    vi.advanceTimersByTime(5000)
    expect(invoke).toHaveBeenCalledTimes(8)
    const failing = inputMarkSettler(vi.fn(() => Promise.reject(new Error('no shell'))))
    failing.poke()
    vi.advanceTimersByTime(3000)
    await Promise.resolve()
  } finally { vi.useRealTimers() }
})

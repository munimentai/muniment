import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import MachinePicker from '../runtimes/MachinePicker.svelte'
import { INPUT_MARK_THROTTLE_MS, inputMarkSettler, landFocus, settleInputMark, shown, takesText } from './focus-landing.js'

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

it('asks the shell to watch for the input-source mark on each poke, at most once per throttle, and to stop on stop', async () => {
  vi.useFakeTimers()
  try {
    const invoke = vi.fn(async () => {})
    const watches = () => invoke.mock.calls.filter(([command]) => command === 'input_mark_watch').length
    const stops = () => invoke.mock.calls.filter(([command]) => command === 'input_mark_unwatch').length
    const settler = inputMarkSettler(invoke)
    expect(INPUT_MARK_THROTTLE_MS).toBe(250)
    // The first poke asks at once, so the shell checks from the first frame.
    settler.poke()
    expect(watches()).toBe(1)
    // A burst of keystrokes and output asks once more, at the burst's end.
    for (let i = 0; i < 20; i++) { settler.poke(); vi.advanceTimersByTime(10) }
    expect(watches()).toBe(1)
    vi.advanceTimersByTime(50)
    expect(watches()).toBe(2)
    vi.advanceTimersByTime(250)
    expect(watches()).toBe(2)
    vi.advanceTimersByTime(10000)
    expect(watches()).toBe(2)
    // A poke after a quiet spell asks at once again.
    settler.poke()
    expect(watches()).toBe(3)
    // A stop ends the watch and drops a pending ask.
    settler.poke()
    settler.stop()
    expect(stops()).toBe(1)
    vi.advanceTimersByTime(1000)
    expect(watches()).toBe(3)
    settler.poke()
    expect(watches()).toBe(4)
    expect(invoke.mock.calls.every(([command]) => ['input_mark_watch', 'input_mark_unwatch'].includes(command))).toBe(true)
    const failing = inputMarkSettler(vi.fn(() => Promise.reject(new Error('no shell'))))
    failing.poke()
    failing.stop()
    await Promise.resolve()
  } finally { vi.useRealTimers() }
})

// A click inside a field: the pointer lands there, then the field takes focus.
function click(field) {
  field.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, composed: true }))
  field.focus()
}

it('asks to hide the mark when focus moves from one text field to another, as into a picker search field', () => {
  document.body.innerHTML = `
    <textarea aria-label="Message"></textarea>
    <button>Model</button>
    <div role="group"><input type="search" aria-label="Search models"><input type="checkbox" aria-label="Pin"></div>`
  const [composer, button, search, pin] = ['textarea', 'button', 'input[type=search]', 'input[type=checkbox]'].map((selector) => document.querySelector(selector))
  const settler = { poke: vi.fn() }
  const stop = settleInputMark(document, settler)
  try {
    // A click into the first text field leaves no mark behind.
    click(composer)
    expect(settler.poke).not.toHaveBeenCalled()
    // The composer's caret hands focus straight to the picker's search field.
    search.focus()
    expect(settler.poke).toHaveBeenCalledTimes(1)
    // A click that moves focus off the text field asks, and a move between
    // controls that take no text asks nothing.
    click(pin)
    expect(settler.poke).toHaveBeenCalledTimes(2)
    click(button)
    expect(settler.poke).toHaveBeenCalledTimes(2)
    // Back to the composer by way of a button: the last text field was the search field.
    click(composer)
    expect(settler.poke).toHaveBeenCalledTimes(3)
    // A blur asks, and a click that returns to the same field asks nothing.
    composer.blur()
    expect(settler.poke).toHaveBeenCalledTimes(4)
    click(composer)
    expect(settler.poke).toHaveBeenCalledTimes(4)
  } finally { stop() }
  search.focus()
  expect(settler.poke).toHaveBeenCalledTimes(4)
})

// Each trigger, through the real settler on fake timers: the count of watch
// requests the shell receives.
function markWatch() {
  vi.useFakeTimers()
  document.body.innerHTML = `
    <aside><button aria-expanded="false" aria-haspopup="menu">This computer</button></aside>
    <main><textarea aria-label="Message"></textarea></main>`
  const invoke = vi.fn(async () => {})
  const watches = () => invoke.mock.calls.filter(([command]) => command === 'input_mark_watch').length
  const composer = document.querySelector('textarea')
  const machines = document.querySelector('button')
  return { invoke, watches, composer, machines, settler: inputMarkSettler(invoke) }
}

it('asks to hide the mark when code focuses the composer, as at launch, on Open agent and on a thread switch', () => {
  const { watches, composer, settler } = markWatch()
  // A relaunch focuses the composer before the listener starts.
  composer.focus()
  const stop = settleInputMark(document, settler)
  try {
    expect(watches()).toBe(1)
    vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
    // "Open agent" and a thread switch move focus off the field, then code
    // focuses it again: the blur asks at once and the focus at the throttle's end.
    composer.blur()
    composer.focus()
    expect(watches()).toBe(2)
    vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
    expect(watches()).toBe(3)
    composer.blur()
    vi.advanceTimersByTime(5000)
    expect(watches()).toBe(4)
    // A plain click into the composer asks nothing.
    click(composer)
    vi.advanceTimersByTime(5000)
    expect(watches()).toBe(4)
  } finally { stop(); vi.useRealTimers() }
})

it('asks to hide the mark when the window becomes active over a focused text field, and not over a button', () => {
  const { watches, composer, machines, settler } = markWatch()
  const stop = settleInputMark(document, settler)
  try {
    click(composer)
    expect(watches()).toBe(0)
    window.dispatchEvent(new FocusEvent('focus'))
    expect(watches()).toBe(1)
    vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
    document.dispatchEvent(new Event('visibilitychange'))
    expect(watches()).toBe(2)
    vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
    // The click onto the button asks once, and activation over it asks nothing.
    click(machines)
    expect(watches()).toBe(3)
    window.dispatchEvent(new FocusEvent('focus'))
    vi.advanceTimersByTime(5000)
    expect(watches()).toBe(3)
  } finally { stop(); vi.useRealTimers() }
})

it('asks to hide the mark when a menu opens over a focused text field', async () => {
  const { watches, composer, machines, settler } = markWatch()
  const stop = settleInputMark(document, settler)
  try {
    click(composer)
    // WebKit gives a clicked button no focus, so the composer keeps it while the machine menu opens.
    machines.setAttribute('aria-expanded', 'true')
    await Promise.resolve()
    expect(watches()).toBe(1)
    vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
    document.querySelector('aside').insertAdjacentHTML('beforeend', '<div><div role="menu" aria-label="Machines"></div></div>')
    await Promise.resolve()
    expect(watches()).toBe(2)
    vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
    // Plain content, such as a streamed reply, asks nothing.
    document.querySelector('main').insertAdjacentHTML('beforeend', '<p>Reply</p>')
    await Promise.resolve()
    vi.advanceTimersByTime(5000)
    expect(watches()).toBe(2)
    // The menu closing over the composer asks again.
    machines.setAttribute('aria-expanded', 'false')
    await Promise.resolve()
    expect(watches()).toBe(3)
    vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
    // A click that moves focus from the composer to a button asks, and the
    // menu that opens then asks again at the throttle's end.
    click(composer)
    click(machines)
    expect(watches()).toBe(4)
    document.querySelector('aside').insertAdjacentHTML('beforeend', '<div role="listbox"></div>')
    await Promise.resolve()
    vi.advanceTimersByTime(5000)
    expect(watches()).toBe(5)
  } finally { stop(); vi.useRealTimers() }
})

// The composer's machine menu, mounted for real beside a focused composer.
// Opening it moves focus to its first item, and Escape closes it onto its button.
async function machineMenu(props) {
  const { invoke, watches, composer, settler } = markWatch()
  document.querySelector('aside').replaceChildren()
  const here = { id: 'local', name: 'This computer', kind: 'local', local: true, active: true, status: 'online' }
  const pod = { id: 'pod', name: 'Pod', kind: 'container', status: 'online' }
  const picker = mount(MachinePicker, { target: document.querySelector('aside'), props: { list: { selected: 'local', runtimes: [here, pod] }, ...props } })
  await tick()
  const chip = document.querySelector('[data-menu="machine"]')
  const settle = async () => { for (let round = 0; round < 3; round += 1) { await tick(); await Promise.resolve() } }
  return { invoke, watches, composer, settler, chip, settle, picker }
}

for (const [state, props] of [['a new thread', {}], ['an open thread', { locked: 'local' }]]) {
  it(`asks to hide the mark when the machine menu of ${state} opens from a focused composer and when Escape closes it`, async () => {
    const { watches, composer, settler, chip, settle, picker } = await machineMenu(props)
    const stop = settleInputMark(document, settler)
    try {
      click(composer)
      expect(watches()).toBe(0)
      // WebKit gives the clicked chip no focus, and the menu then focuses its first item.
      chip.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, composed: true }))
      chip.click()
      await settle()
      const menu = document.querySelector('[role="menu"][aria-label="Machines"]')
      expect(menu).toBeTruthy()
      expect(menu.contains(document.activeElement)).toBe(true)
      expect(watches()).toBe(1)
      // The menu opening and its first item taking focus share one request
      // and one more at the throttle's end. The menu then stays open with
      // nothing more to ask: the watch hid the mark.
      vi.advanceTimersByTime(INPUT_MARK_THROTTLE_MS)
      const opened = watches()
      vi.advanceTimersByTime(20_000)
      expect(watches()).toBe(opened)
      document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      await settle()
      expect(document.querySelector('[role="menu"][aria-label="Machines"]')).toBeNull()
      expect(document.activeElement).toBe(chip)
      expect(watches()).toBe(opened + 1)
      vi.advanceTimersByTime(20_000)
      expect(watches()).toBe(opened + 1)
      // Back in the composer by a click, the menu asks again as it opens.
      click(composer)
      chip.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, composed: true }))
      chip.click()
      await settle()
      expect(watches()).toBe(opened + 2)
    } finally { stop(); unmount(picker); vi.useRealTimers() }
  })
}

it('asks to hide the mark when a menu closes back to a composer that kept focus', async () => {
  const { watches, composer, machines, settler } = markWatch()
  const stop = settleInputMark(document, settler)
  try {
    click(composer)
    machines.setAttribute('aria-expanded', 'true')
    document.querySelector('aside').insertAdjacentHTML('beforeend', '<div role="menu" aria-label="Machines"></div>')
    await Promise.resolve()
    expect(watches()).toBe(1)
    vi.advanceTimersByTime(5000)
    machines.setAttribute('aria-expanded', 'false')
    document.querySelector('[role="menu"]').remove()
    await Promise.resolve()
    expect(document.activeElement).toBe(composer)
    expect(watches()).toBe(2)
    vi.advanceTimersByTime(5000)
    // An unchanged aria-expanded asks nothing.
    machines.setAttribute('aria-expanded', 'false')
    await Promise.resolve()
    vi.advanceTimersByTime(5000)
    expect(watches()).toBe(2)
  } finally { stop(); vi.useRealTimers() }
})

// Each way focus leaves the composer with no text field to take it, through
// the real settler on fake timers. AppKit keeps the mark beside a caret the
// user clicked, as TextEdit does, so the click into the composer asks nothing
// and each departure asks once.
describe('the mark after focus leaves the composer', () => {
  const departures = [
    ['a click on Save', ({ machines }) => click(machines)],
    ['a click on Send', ({ send }) => click(send)],
    ['a blur to the page', ({ composer }) => composer.blur()],
    ['code that focuses a heading', ({ heading }) => heading.focus()],
    ['an agent delete that removes the composer', ({ composer }) => composer.closest('main').remove()],
    ['a composer that turns disabled', ({ composer }) => { composer.disabled = true }],
  ]
  for (const [name, leave] of departures) {
    it(`asks to hide the mark after ${name}`, async () => {
      const parts = markWatch()
      const { watches, composer, settler } = parts
      document.querySelector('main').insertAdjacentHTML('beforeend', '<button>Send</button><h1 tabindex="-1">Agents</h1>')
      const send = document.querySelector('main button')
      const heading = document.querySelector('h1')
      const stop = settleInputMark(document, settler)
      try {
        click(composer)
        vi.advanceTimersByTime(12_000)
        expect(watches()).toBe(0)
        leave({ ...parts, send, heading })
        await Promise.resolve()
        expect(watches()).toBe(1)
        vi.advanceTimersByTime(20_000)
        expect(watches()).toBe(1)
      } finally { stop(); vi.useRealTimers() }
    })
  }

  it('asks nothing when the window loses focus over the composer', () => {
    const { watches, composer, settler } = markWatch()
    const stop = settleInputMark(document, settler)
    try {
      click(composer)
      // A window blur leaves the composer as the active element.
      composer.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: null }))
      vi.advanceTimersByTime(5000)
      expect(watches()).toBe(0)
    } finally { stop(); vi.useRealTimers() }
  })
})

import { afterEach, expect, it, vi } from 'vitest'
import { userEvent } from 'vitest/browser'
import { flushSync, mount, tick, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import TerminalWorkspace from './TerminalWorkspace.svelte'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
})

const LONG = '/Users/mpruitt/Documents/muniment/sessions/Single-Word-Pong-Request/workspace'
// What zsh writes as it starts under the terminal's own prompt: the partial
// line mark, then the `$ ` prompt.
const PROMPT = '\x1b[1m\x1b[7m%\x1b[27m\x1b[1m\x1b[0m' + ' '.repeat(79) + '\r \r\r\x1b[0m\x1b[27m\x1b[24m\x1b[J$ \x1b[K\x1b[?2004h'
const settle = async () => { await tick(); await new Promise((resolve) => requestAnimationFrame(resolve)); await new Promise((resolve) => setTimeout(resolve, 50)) }

// The Terminal panel at `width` pixels, open on `path`, with a shell that
// answers `output` on its first read.
async function terminal(width, path, output = '') {
  const target = document.createElement('div')
  Object.assign(target.style, { position: 'absolute', left: '20px', top: '20px', width: `${width}px`, height: '400px', display: 'flex', flexDirection: 'column' })
  document.body.append(target)
  const calls = []
  let pending = new TextEncoder().encode(output)
  const invoke = async (command, args) => {
    calls.push([command, args])
    if (command === 'workspace_folders') return [{ label: 'Session', path }]
    if (command === 'terminal_start') return 'session-1'
    if (command === 'terminal_read') { const bytes = [...pending]; pending = new Uint8Array(); return { bytes, exited: false } }
    return null
  }
  component = mount(TerminalWorkspace, { target, props: { tauri: { invoke } } })
  for (let i = 0; i < 20 && !calls.some(([command]) => command === 'terminal_read'); i += 1) { await new Promise((resolve) => setTimeout(resolve, 20)); flushSync() }
  await settle()
  const send = (text) => { pending = new TextEncoder().encode(text) }
  return { target, calls, send, line: target.querySelector('header .path') }
}

it('drops the start of a long terminal path behind a leading ellipsis and names the full path on hover', async () => {
  const { line } = await terminal(260, LONG)
  const text = line.querySelector('.path-text')
  const style = getComputedStyle(text)
  expect(style.textOverflow).toBe('ellipsis')
  expect(style.direction).toBe('rtl')
  expect(style.maskImage).toBe('none')
  expect(text.scrollWidth).toBeGreaterThan(text.clientWidth)
  // The end of the path shows: its last character sits inside the line.
  const range = document.createRange()
  const node = text.querySelector('bdi').firstChild
  range.setStart(node, LONG.length - 1)
  range.setEnd(node, LONG.length)
  const box = text.getBoundingClientRect()
  const last = range.getBoundingClientRect()
  expect(last.right).toBeLessThanOrEqual(box.right + 0.5)
  expect(last.left).toBeGreaterThanOrEqual(box.left)
  expect(text.textContent).toBe(LONG)
  expect(line.dataset.cut).toBe('true')
  expect(line.dataset.tooltip).toBe(LONG)
  await userEvent.hover(line)
  await expect.poll(() => getComputedStyle(line, '::after').opacity).toBe('1')
  expect(getComputedStyle(line, '::after').content).toBe(`"${LONG}"`)
})

it('shows a short terminal path from its start with no hover name', async () => {
  const { line } = await terminal(600, '/tmp/work')
  const text = line.querySelector('.path-text')
  expect(text.scrollWidth).toBeLessThanOrEqual(text.clientWidth)
  expect(line.dataset.cut).toBe('false')
  expect(getComputedStyle(line, '::after').content).toBe('none')
})

it('gives the shell the panel size it grew to while the shell started, so the prompt fits', async () => {
  const target = document.createElement('div')
  Object.assign(target.style, { position: 'absolute', left: '20px', top: '20px', width: '0px', height: '400px', display: 'flex', flexDirection: 'column' })
  document.body.append(target)
  const calls = []
  let release
  const started = new Promise((resolve) => { release = resolve })
  const invoke = async (command, args) => {
    calls.push([command, args])
    if (command === 'workspace_folders') return [{ label: 'Session', path: '/tmp/work' }]
    if (command === 'terminal_start') { await started; return 'session-1' }
    if (command === 'terminal_read') return { bytes: [], exited: false }
    return null
  }
  component = mount(TerminalWorkspace, { target, props: { tauri: { invoke } } })
  await expect.poll(() => calls.some(([command]) => command === 'terminal_start')).toBe(true)
  const narrow = calls.find(([command]) => command === 'terminal_start')[1]
  // The panel finishes its slide before the shell answers with its id.
  target.style.width = '600px'
  await settle()
  release()
  await expect.poll(() => calls.filter(([command]) => command === 'terminal_resize').at(-1)?.[1]?.cols ?? 0).toBeGreaterThan(narrow.cols)
  const sized = calls.filter(([command]) => command === 'terminal_resize').at(-1)[1]
  expect(sized.id).toBe('session-1')
  expect(sized.cols).toBeGreaterThan(20)
})

it('draws the shell prompt as soon as the terminal opens', async () => {
  const { target } = await terminal(600, '/tmp/work', PROMPT)
  await expect.poll(() => target.querySelector('.xterm-rows')?.textContent ?? '').toContain('$')
})

it('insets the prompt from the panel edge as far as the header path, so the two line up', async () => {
  const { target, line } = await terminal(600, '/tmp/work', PROMPT)
  await expect.poll(() => target.querySelector('.xterm-rows')?.textContent ?? '').toContain('$')
  const panel = target.querySelector('.terminal-workspace').getBoundingClientRect()
  const prompt = [...target.querySelectorAll('.xterm-rows > div')].find((row) => row.textContent.includes('$')).getBoundingClientRect()
  expect(prompt.left - panel.left).toBeGreaterThanOrEqual(12)
  expect(prompt.left).toBe(line.getBoundingClientRect().left)
})

it('hides the input-source mark a second after the terminal takes focus and after each keystroke', async () => {
  const { target, calls } = await terminal(600, '/tmp/work', PROMPT)
  await expect.poll(() => target.querySelector('.xterm-rows')?.textContent ?? '').toContain('$')
  const dismissed = () => calls.filter(([command]) => command === 'input_mark_dismiss').length
  const field = target.querySelector('.xterm-helper-textarea')
  // The shell's start focused the terminal: let that wait run out, then focus it again.
  field.blur()
  await new Promise((resolve) => setTimeout(resolve, 1100))
  const before = dismissed()
  expect(before).toBeGreaterThanOrEqual(1)
  calls.length = 0
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  try {
    field.focus()
    vi.advanceTimersByTime(999)
    expect(dismissed()).toBe(0)
    vi.advanceTimersByTime(1)
    expect(dismissed()).toBe(1)
    await userEvent.keyboard('ls')
    vi.advanceTimersByTime(500)
    expect(dismissed()).toBe(1)
    await userEvent.keyboard('{Enter}')
    vi.advanceTimersByTime(999)
    expect(dismissed()).toBe(1)
    vi.advanceTimersByTime(1)
    expect(dismissed()).toBe(2)
  } finally { vi.useRealTimers() }
})

it('hides the input-source mark a second after shell output settles under a new prompt', async () => {
  const { target, calls, send } = await terminal(600, '/tmp/work', PROMPT)
  await expect.poll(() => target.querySelector('.xterm-rows')?.textContent ?? '').toContain('$')
  const dismissed = () => calls.filter(([command]) => command === 'input_mark_dismiss').length
  const field = target.querySelector('.xterm-helper-textarea')
  field.focus()
  await new Promise((resolve) => setTimeout(resolve, 1100))
  calls.length = 0
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  try {
    // The command's output and the next prompt arrive in two reads, and AppKit
    // shows the mark again as the caret moves with them.
    send('final-pass\r\n')
    await vi.advanceTimersByTimeAsync(100)
    await expect.poll(() => target.querySelector('.xterm-rows')?.textContent ?? '').toContain('final-pass')
    send('$ ')
    await vi.advanceTimersByTimeAsync(600)
    expect(dismissed()).toBe(0)
    await vi.advanceTimersByTimeAsync(1000)
    expect(dismissed()).toBe(1)
    await vi.advanceTimersByTimeAsync(5000)
    expect(dismissed()).toBe(1)
    // Output to a terminal without focus asks nothing.
    field.blur()
    send('later\r\n$ ')
    await vi.advanceTimersByTimeAsync(3000)
    expect(dismissed()).toBe(1)
  } finally { vi.useRealTimers() }
})

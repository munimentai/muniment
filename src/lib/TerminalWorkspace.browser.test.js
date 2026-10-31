import { afterEach, expect, it } from 'vitest'
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
  return { target, calls, line: target.querySelector('header .path') }
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

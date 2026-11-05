import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { flushSync, mount, tick, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import FilesWorkspace from './FilesWorkspace.svelte'

let component
afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.replaceChildren()
  localStorage.clear()
})

const LONG = '/Users/mpruitt/Documents/muniment/sessions/Single-Word-Pong-Request/workspace'

// The Files panel at `width` pixels, open on `path`.
async function files(width, path) {
  const target = document.createElement('div')
  Object.assign(target.style, { position: 'absolute', left: '20px', top: '20px', width: `${width}px`, height: '400px', display: 'flex', flexDirection: 'column' })
  document.body.append(target)
  const invoke = async (command, args) => command === 'workspace_folders' ? [{ label: 'Session', path }] : { path: args.path, entries: [] }
  component = mount(FilesWorkspace, { target, props: { tauri: { invoke } } })
  for (let i = 0; i < 20 && !target.querySelector('.path'); i += 1) { await new Promise((resolve) => setTimeout(resolve, 20)); flushSync() }
  await tick()
  await new Promise((resolve) => requestAnimationFrame(resolve))
  return target.querySelector('.path')
}

it('drops the start of a long path behind a leading ellipsis and names the full path on hover', async () => {
  const line = await files(260, LONG)
  const text = line.querySelector('.path-text')
  const style = getComputedStyle(text)
  expect(style.textOverflow).toBe('ellipsis')
  expect(style.direction).toBe('rtl')
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
  // The path keeps its own reading order.
  expect(text.textContent).toBe(LONG)
  expect(line.dataset.cut).toBe('true')
  expect(line.dataset.tooltip).toBe(LONG)
  await userEvent.hover(line)
  await expect.poll(() => getComputedStyle(line, '::after').opacity).toBe('1')
  expect(getComputedStyle(line, '::after').content).toBe(`"${LONG}"`)
})

it('shows a short path from its start with no hover name', async () => {
  const line = await files(600, '/tmp/work')
  const text = line.querySelector('.path-text')
  expect(text.scrollWidth).toBeLessThanOrEqual(text.clientWidth)
  const range = document.createRange()
  range.selectNodeContents(text.querySelector('bdi'))
  expect(Math.round(range.getBoundingClientRect().left)).toBe(Math.round(text.getBoundingClientRect().left + parseFloat(getComputedStyle(text).paddingLeft)))
  expect(line.dataset.cut).toBe('false')
  expect(getComputedStyle(line, '::after').content).toBe('none')
})

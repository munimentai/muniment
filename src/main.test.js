import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { mount } from 'svelte'
// Compile the entry point during collection, outside the per-test timeout.
// Each test re-evaluates its startup code after resetModules().
import './main.js'

vi.mock('svelte', () => ({ mount: vi.fn() }))
vi.mock('./App.svelte', () => ({ default: {} }))
vi.mock('./Launcher.svelte', () => ({ default: {} }))
vi.mock('./lib/WorkspacePopup.svelte', () => ({ default: {} }))

beforeEach(() => {
  vi.resetModules()
  vi.resetAllMocks()
})

afterEach(() => {
  history.replaceState({}, '', '/')
  localStorage.clear()
  delete document.documentElement.dataset.theme
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

it.each([
  ['light', 'moss-light'],
  ['dark', 'moss-dark'],
  ['moss', 'moss-dark'],
  ['iris-contrast-dark', 'iris-contrast-dark'],
  ['{"mode":"light","palette":"harbor"}', 'harbor-light'],
  ['{"mode":"dark","palette":"clay-vibrant"}', 'clay-vibrant-dark'],
  ['{"mode":"system","palette":"iris"}', 'iris-light'],
  ['{"mode":"light","light":"ledger","dark":"vault"}', 'harbor-light'],
  ['{"mode":"dark","light":"ledger","dark":"vault"}', 'moss-dark'],
  ['{"mode":"system","palette":"moss"}', 'moss-light'],
  ['{"mode":"system","palette":"moss-vibrant"}', undefined],
  ['system', 'moss-light'],
  [null, 'moss-vibrant-dark'],
  ['', 'moss-vibrant-dark'],
  ['Dark', 'moss-vibrant-dark'],
  ['{"theme":"dark"}', 'moss-dark'],
])('applies the stored theme %j before the app mounts at launch', async (stored, expected) => {
  if (stored !== null) localStorage.setItem('muniment.theme', stored)
  document.documentElement.dataset.theme = 'stale'
  document.body.innerHTML = '<div id="app"></div>'
  mount.mockImplementation((_component, { target }) => {
    expect(target).toBe(document.getElementById('app'))
    expect(document.documentElement.dataset.theme).toBe(expected)
  })

  await import('./main.js')
  expect(mount).toHaveBeenCalledOnce()
})

it('mounts only the launcher in the launcher window', async () => {
  history.replaceState({}, '', '/index.html?launcher')
  document.body.innerHTML = '<div id="app"></div>'
  const { default: Launcher } = await import('./Launcher.svelte')
  await import('./main.js')
  expect(mount).toHaveBeenCalledExactlyOnceWith(Launcher, { target: document.getElementById('app') })
})

it('uses Moss vibrant dark at launch when storage is unavailable', async () => {
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('Storage unavailable') })
  document.documentElement.dataset.theme = 'stale'
  mount.mockImplementation(() => {
    expect(document.documentElement.dataset.theme).toBe('moss-vibrant-dark')
  })

  await import('./main.js')
  expect(mount).toHaveBeenCalledOnce()
})

it('shows the hidden main window after the app mounts, with the page background, and never from the launcher', async () => {
  const invoke = vi.fn(async () => {})
  window.__TAURI__ = { core: { invoke } }
  try {
    document.body.innerHTML = '<div id="app"></div>'
    document.body.style.background = 'rgb(11, 28, 23)'
    mount.mockImplementation(() => { expect(invoke).not.toHaveBeenCalledWith('main_window_ready', expect.anything()) })
    await import('./main.js')
    expect(invoke).toHaveBeenCalledWith('main_window_ready', { background: 'rgb(11, 28, 23)' })
    // A theme change sends the new background.
    document.body.style.background = 'rgb(228, 244, 236)'
    window.dispatchEvent(new CustomEvent('muniment:theme'))
    expect(invoke).toHaveBeenLastCalledWith('main_window_ready', { background: 'rgb(228, 244, 236)' })

    invoke.mockClear()
    vi.resetModules()
    history.replaceState({}, '', '/index.html?launcher')
    await import('./main.js')
    expect(invoke).not.toHaveBeenCalledWith('main_window_ready', expect.anything())
  } finally {
    delete window.__TAURI__
    document.body.removeAttribute('style')
  }
})

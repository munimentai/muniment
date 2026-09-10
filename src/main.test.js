import { afterEach, expect, it, vi } from 'vitest'
import { mount } from 'svelte'

vi.mock('svelte', () => ({ mount: vi.fn() }))
vi.mock('./App.svelte', () => ({ default: {} }))

afterEach(() => {
  localStorage.clear()
  delete document.documentElement.dataset.theme
  document.body.innerHTML = ''
  vi.restoreAllMocks()
  vi.resetAllMocks()
})

it.each([
  ['light', 'light'],
  ['dark', 'dark'],
  ['system', undefined],
  [null, undefined],
  ['', undefined],
  ['Dark', undefined],
  ['{"theme":"dark"}', undefined],
])('applies the stored theme %j before the app mounts at launch', async (stored, expected) => {
  vi.resetModules()
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

it('uses the system theme at launch when storage is unavailable', async () => {
  vi.resetModules()
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('Storage unavailable') })
  document.documentElement.dataset.theme = 'dark'
  mount.mockImplementation(() => {
    expect(document.documentElement.dataset.theme).toBeUndefined()
  })

  await import('./main.js')
  expect(mount).toHaveBeenCalledOnce()
})

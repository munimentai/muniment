import { afterEach, expect, it, vi } from 'vitest'
import { accountCache } from './account-cache.js'
afterEach(() => vi.useRealTimers())

it('keeps a snapshot across settings mounts and polls while the shell owns it', async () => {
  vi.useFakeTimers()
  const settings = { accounts: [{ id: 'one', allowance_readable: true, windows: [{ remaining_percent: 70 }] }] }
  const bridge = { invoke: vi.fn(async () => settings) }
  const cache = accountCache(bridge)
  const shell = cache.start()
  await vi.advanceTimersByTimeAsync(0)
  expect(cache.current.settings.accounts[0].windows[0].remaining_percent).toBe(70)
  const settingsPage = accountCache(bridge)
  expect(settingsPage.current.settings).toBe(cache.current.settings)
  const close = settingsPage.start()
  close()
  bridge.invoke.mockClear()
  await vi.advanceTimersByTimeAsync(120_000)
  expect(bridge.invoke).toHaveBeenCalledWith('model_router_refresh_quota', { id: null })
  shell()
  bridge.invoke.mockClear()
  await vi.advanceTimersByTimeAsync(240_000)
  expect(bridge.invoke).not.toHaveBeenCalled()
})

it('retains last values after failure and does not let an old read undo an account edit', async () => {
  const bridge = { invoke: vi.fn() }
  const cache = accountCache(bridge)
  const saved = { accounts: [{ id: 'saved' }] }
  cache.set(saved)
  bridge.invoke.mockRejectedValueOnce(new Error('offline'))
  await cache.read()
  expect(cache.current.settings).toBe(saved)
  expect(cache.current.error).toBeTruthy()
  let finish
  bridge.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const pending = cache.read()
  await Promise.resolve()
  const edited = { accounts: [] }
  cache.set(edited)
  finish(saved)
  await pending
  expect(cache.current.settings).toBe(edited)
})

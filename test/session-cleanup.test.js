import { afterEach, expect, it, vi } from 'vitest'
import { revokeFixtureSession, waitForFixtureService } from './e2e/support/session-cleanup.mjs'

afterEach(() => { vi.unstubAllGlobals() })

it('revokes the persisted fixture session without requiring a visible profile control', async () => {
  let signedIn = true
  const invoke = vi.fn(async (command) => {
    if (command === 'auth_sign_out') signedIn = false
    return { signed_in: signedIn }
  })
  vi.stubGlobal('window', { __TAURI__: { core: { invoke } } })
  await revokeFixtureSession({ execute: (callback) => callback() })
  expect(invoke.mock.calls).toEqual([['auth_status'], ['auth_sign_out'], ['auth_status']])
})

it('fails cleanup when the fixture remains signed in', async () => {
  await expect(revokeFixtureSession({ execute: async () => ({ signed_in: true }) })).rejects.toThrow('confirm sign-out')
})

it('does not treat an unavailable authentication service as proof of cleanup', async () => {
  await expect(revokeFixtureSession({ execute: async () => { throw new Error('disconnected') } })).rejects.toThrow('disconnected')
})

it('confirms an already signed-out fixture without requesting another sign-out', async () => {
  const invoke = vi.fn(async () => ({ signed_in: false }))
  vi.stubGlobal('window', { __TAURI__: { core: { invoke } } })
  await revokeFixtureSession({ execute: (callback) => callback() })
  expect(invoke.mock.calls).toEqual([['auth_status'], ['auth_status']])
})


it('waits for the desktop connection before fixture cleanup', async () => {
  const invoke = vi.fn()
    .mockResolvedValueOnce({ supervisor_running: false, connected: false })
    .mockResolvedValueOnce({ supervisor_running: true, connected: false })
    .mockResolvedValueOnce({ supervisor_running: true, connected: true })
  vi.stubGlobal('window', { __TAURI__: { core: { invoke } } })
  const waitUntil = vi.fn(async (poll, options) => {
    expect(options.timeout).toBe(60000)
    expect(await poll()).toBe(false)
    expect(await poll()).toBe(false)
    expect(await poll()).toBe(true)
  })
  await waitForFixtureService({ execute: (callback) => callback(), waitUntil })
  expect(invoke.mock.calls).toEqual(Array(3).fill(['attach_listener_status']))
})

it('fails when the desktop does not connect within the cleanup budget', async () => {
  const waitUntil = vi.fn(async (_poll, options) => { throw new Error(options.timeoutMsg) })
  await expect(waitForFixtureService({ waitUntil })).rejects.toThrow('did not connect before fixture cleanup')
})

import { expect, it, vi } from 'vitest'
import { listenForLauncher } from './launcher-bridge.js'

it('waits for startup and returns the submission result to the launcher', async () => {
  let handler
  let finishStartup
  const startup = new Promise((resolve) => { finishStartup = resolve })
  const send = vi.fn().mockResolvedValue(true)
  const emitTo = vi.fn()
  const stop = vi.fn()
  expect(await listenForLauncher({
    listen: async (name, callback) => { expect(name).toBe('launcher-submit'); handler = callback; return stop },
    emitTo, ready: () => startup, send,
  })).toBe(stop)
  const request = handler({ payload: { id: 'one', text: 'Hello' } })
  expect(send).not.toHaveBeenCalled()
  await handler({ payload: { id: 'two', text: 'Concurrent' } })
  expect(emitTo).toHaveBeenCalledWith('launcher', 'launcher-result', expect.objectContaining({ id: 'two', error: expect.stringContaining('Wait') }))
  finishStartup(true)
  await request
  expect(send).toHaveBeenCalledExactlyOnceWith('Hello')
  expect(emitTo).toHaveBeenLastCalledWith('launcher', 'launcher-result', { id: 'one', error: '' })
})

it.each([false, true])('returns an error when startup or submission rejects the request (%s)', async (ready) => {
  let handler
  const emitTo = vi.fn()
  const send = vi.fn().mockResolvedValue(false)
  await listenForLauncher({ listen: async (_, callback) => { handler = callback }, emitTo, ready: async () => ready, send })
  await handler({ payload: { id: 'request', text: 'Hello' } })
  expect(emitTo.mock.calls[0][2].error).not.toBe('')
  expect(send).toHaveBeenCalledTimes(ready ? 1 : 0)
  emitTo.mockClear()
  await handler({ payload: { id: 5, text: null } })
  expect(emitTo).not.toHaveBeenCalled()
})

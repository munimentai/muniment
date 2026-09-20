import { beforeEach, expect, it, vi } from 'vitest'
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke, Channel: class {} }))
import { register, unregister } from './native-shortcuts.js'
beforeEach(() => invoke.mockReset())
it('routes shortcut events through an app command without the plugin lock', async () => {
  const callback = vi.fn()
  await register('Control+Space', callback)
  const [command, args] = invoke.mock.calls[0]
  expect(command).toBe('shortcut_register')
  expect(args.shortcut).toBe('Control+Space')
  args.handler.onmessage({ state: 'Pressed' })
  expect(callback).toHaveBeenCalledWith({ state: 'Pressed' })
})
it('propagates registration failures and unregisters the same shortcut', async () => {
  invoke.mockRejectedValueOnce(new Error('occupied'))
  await expect(register('Control+Space', vi.fn())).rejects.toThrow('occupied')
  await unregister('Control+Space')
  expect(invoke).toHaveBeenLastCalledWith('shortcut_unregister', { shortcut: 'Control+Space' })
})

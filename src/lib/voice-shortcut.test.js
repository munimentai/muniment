import { describe, expect, it, vi } from 'vitest'

import { createVoiceShortcutManager, VOICE_SHORTCUT_STORAGE_KEY } from './voice-shortcut.js'

function deferred() {
  let resolve
  const promise = new Promise((settle) => { resolve = settle })
  return { promise, resolve }
}

function setup({ saved = null, register = vi.fn().mockResolvedValue(), unregister = vi.fn().mockResolvedValue() } = {}) {
  const values = new Map(saved === null ? [] : [[VOICE_SHORTCUT_STORAGE_KEY, saved]])
  const storage = {
    getItem: vi.fn((key) => values.get(key) ?? null),
    setItem: vi.fn((key, value) => values.set(key, value)),
    removeItem: vi.fn((key) => values.delete(key)),
  }
  const states = []
  const onShortcut = vi.fn()
  const manager = createVoiceShortcutManager({
    register,
    unregister,
    storage,
    onShortcut,
    onState: (state) => states.push({ ...state }),
  })
  return { manager, register, unregister, storage, states, values, onShortcut }
}

describe('voice shortcut manager', () => {
  it('falls back to the platform default when the saved shortcut cannot register', async () => {
    const register = vi.fn(async (shortcut) => {
      if (shortcut === 'Alt+Shift+K') throw new Error('collision')
    })
    const context = setup({ saved: 'Alt+Shift+K', register })

    await context.manager.start()

    expect(register.mock.calls).toEqual([
      ['Alt+Shift+K', context.onShortcut],
      ['Control+Shift+Space', context.onShortcut],
    ])
    expect(context.states.at(-1)).toMatchObject({
      shortcut: 'Control+Shift+Space',
      registered: true,
      changing: false,
      error: false,
    })
  })

  it('restores the registered shortcut and persisted value when a change fails', async () => {
    const previousUnregistration = deferred()
    const unregister = vi.fn(async (shortcut) => {
      if (shortcut === 'Alt+Shift+K') await previousUnregistration.promise
    })
    const context = setup({ saved: 'Alt+Shift+K', unregister })
    await context.manager.start()

    const change = context.manager.change('Control+Alt+K')
    await vi.waitFor(() => expect(unregister).toHaveBeenCalledWith('Alt+Shift+K'))
    const cleanup = context.manager.cleanup()
    previousUnregistration.resolve()
    expect(await change).toBe(false)
    await cleanup

    expect(context.register.mock.calls.map(([shortcut]) => shortcut)).toEqual([
      'Alt+Shift+K',
      'Control+Alt+K',
      'Alt+Shift+K',
    ])
    expect(context.values.get(VOICE_SHORTCUT_STORAGE_KEY)).toBe('Alt+Shift+K')
  })

  it('unregisters every shortcut still tracked during cleanup', async () => {
    const unregister = vi.fn().mockRejectedValue(new Error('still registered'))
    const context = setup({ unregister })
    await context.manager.start()
    expect(await context.manager.change('Control+Alt+K')).toBeNull()

    unregister.mockClear()
    await context.manager.cleanup()

    expect(unregister.mock.calls.map(([shortcut]) => shortcut)).toEqual([
      'Control+Shift+Space',
      'Control+Alt+K',
    ])
  })

  it('serializes overlapping change requests on the task chain', async () => {
    const firstRegistration = deferred()
    const calls = []
    const register = vi.fn(async (shortcut) => {
      calls.push(`register:${shortcut}`)
      if (shortcut === 'Control+Alt+K') await firstRegistration.promise
    })
    const unregister = vi.fn(async (shortcut) => { calls.push(`unregister:${shortcut}`) })
    const context = setup({ register, unregister })
    await context.manager.start()

    const first = context.manager.change('Control+Alt+K')
    const second = context.manager.change('Alt+Shift+J')
    await Promise.resolve()
    expect(calls).not.toContain('register:Alt+Shift+J')

    firstRegistration.resolve()
    await expect(Promise.all([first, second])).resolves.toEqual([true, true])
    expect(calls).toEqual([
      'register:Control+Shift+Space',
      'register:Control+Alt+K',
      'unregister:Control+Shift+Space',
      'register:Alt+Shift+J',
      'unregister:Control+Alt+K',
    ])
    expect(context.states.at(-1)).toMatchObject({ shortcut: 'Alt+Shift+J', changing: false })
  })
})

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Launcher from './Launcher.svelte'

let handlers
let invoke
let emitTo
let stops
beforeEach(() => {
  localStorage.clear()
  delete document.documentElement.dataset.theme
  handlers = {}
  stops = []
  invoke = vi.fn().mockResolvedValue(undefined)
  emitTo = vi.fn().mockResolvedValue(undefined)
  window.__TAURI__ = {
    core: { invoke },
    event: {
      emitTo,
      listen: vi.fn(async (name, handler) => {
        handlers[name] = handler
        const stop = vi.fn()
        stops.push(stop)
        return stop
      }),
    },
  }
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  delete document.documentElement.dataset.theme
  delete window.__TAURI__
})

async function open() {
  render(Launcher)
  await waitFor(() => expect(handlers['launcher-result']).toBeTypeOf('function'))
  handlers['launcher-opened']()
  const input = screen.getByRole('textbox', { name: 'First message' })
  expect(document.activeElement).toBe(input)
  return input
}

async function typeAndSend(input, text = '  Organize my notes  ') {
  await fireEvent.input(input, { target: { value: text } })
  await fireEvent.keyDown(input, { key: 'Enter' })
}

function reply(error = '') {
  const { id } = emitTo.mock.calls.at(-1)[2]
  return handlers['launcher-result']({ payload: { id, error } })
}

describe('launcher', () => {
  it('starts with one input and no alert on a fresh profile', async () => {
    const input = await open()
    expect(screen.getAllByRole('textbox')).toEqual([input])
    expect(input.value).toBe('')
    expect(input.getAttribute('aria-invalid')).toBe('false')
    expect(screen.getByRole('alert').textContent).toBe('')
    expect(input.checkValidity()).toBe(true)
    expect(invoke).not.toHaveBeenCalledWith('launcher_start_failed', expect.anything())
  })

  it.each([new Error('event listener denied'), 'event listener denied'])(
    'shows and logs the start failure cause %s', async (failure) => {
      const log = vi.spyOn(console, 'error').mockImplementation(() => {})
      window.__TAURI__.event.listen.mockRejectedValueOnce(failure)
      try {
        render(Launcher)
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('launcher_start_failed', { cause: 'event listener denied' }))
        const message = 'The launcher could not start: event listener denied. Restart the app.'
        expect(screen.getByRole('alert').textContent).toBe(message)
        expect(log).toHaveBeenCalledWith(message)
        const input = screen.getByRole('textbox', { name: 'First message' })
        expect(input.getAttribute('aria-invalid')).toBe('true')
        await typeAndSend(input)
        expect(emitTo).not.toHaveBeenCalled()
      } finally {
        log.mockRestore()
      }
    },
  )

  it('removes the first listener when the second listener fails and handles a log failure', async () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const stop = vi.fn().mockRejectedValue(new Error('unlisten unavailable'))
    window.__TAURI__.event.listen.mockResolvedValueOnce(stop).mockRejectedValueOnce('second listener denied')
    invoke.mockRejectedValueOnce(new Error('log unavailable'))
    try {
      render(Launcher)
      await waitFor(() => expect(log).toHaveBeenCalledWith('The launcher could not log its start failure.', expect.any(Error)))
      expect(stop).toHaveBeenCalledTimes(1)
      expect(screen.getByRole('alert').textContent).toContain('second listener denied')
      cleanup()
      expect(stop).toHaveBeenCalledTimes(1)
    } finally {
      log.mockRestore()
    }
  })

  it('removes a listener that resolves after the launcher unmounts', async () => {
    let resolve
    const stop = vi.fn()
    window.__TAURI__.event.listen.mockReturnValueOnce(new Promise((done) => { resolve = done }))
    render(Launcher)
    cleanup()
    resolve(stop)
    await waitFor(() => expect(stop).toHaveBeenCalledTimes(1))
    expect(window.__TAURI__.event.listen).toHaveBeenCalledTimes(1)
    expect(invoke).not.toHaveBeenCalled()
  })

  it('syncs Light, Dark, and System from another window while hidden and on reopen', async () => {
    const input = await open()
    for (const theme of ['light', 'dark', 'system', 'dark', 'light']) {
      await fireEvent.keyDown(input, { key: 'Escape' })
      localStorage.setItem('muniment.theme', theme)
      await fireEvent(window, new StorageEvent('storage', {
        key: 'muniment.theme', newValue: theme, storageArea: localStorage,
      }))
      expect(document.documentElement.dataset.theme).toBe(theme === 'system' ? undefined : theme)
      // Reopening repairs a missed storage event.
      document.documentElement.dataset.theme = 'stale'
      handlers['launcher-opened']()
      expect(document.documentElement.dataset.theme).toBe(theme === 'system' ? undefined : theme)
    }
    localStorage.clear()
    await fireEvent(window, new StorageEvent('storage', { key: null, storageArea: localStorage }))
    expect(document.documentElement.dataset.theme).toBeUndefined()
  })

  it('uses System for malformed or unavailable storage and ignores unrelated changes', async () => {
    localStorage.setItem('muniment.theme', 'dark')
    await open()
    expect(document.documentElement.dataset.theme).toBe('dark')
    localStorage.setItem('muniment.theme', 'invalid')
    await fireEvent(window, new StorageEvent('storage', { key: 'other', storageArea: localStorage }))
    expect(document.documentElement.dataset.theme).toBe('dark')
    handlers['launcher-opened']()
    expect(document.documentElement.dataset.theme).toBeUndefined()
    const read = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('denied') })
    document.documentElement.dataset.theme = 'dark'
    handlers['launcher-opened']()
    expect(document.documentElement.dataset.theme).toBeUndefined()
    read.mockRestore()
  })

  it('opens, sends once, presents the main window, and closes with Escape', async () => {
    const input = await open()
    await typeAndSend(input)
    expect(emitTo).toHaveBeenCalledWith('main', 'launcher-submit', {
      id: expect.any(String), text: 'Organize my notes',
    })
    await fireEvent.keyDown(input, { key: 'Enter' })
    expect(emitTo).toHaveBeenCalledTimes(1)
    await reply()
    await waitFor(() => expect(input.value).toBe(''))
    expect(invoke).toHaveBeenCalledWith('launcher_present_main')
    handlers['launcher-opened']()
    await fireEvent.keyDown(input, { key: 'Escape' })
    expect(invoke).toHaveBeenCalledWith('launcher_close')
    cleanup()
    expect(stops.every((stop) => stop.mock.calls.length === 1)).toBe(true)
  })

  it('ignores blank input and composition keys', async () => {
    const input = await open()
    await typeAndSend(input, '   ')
    await fireEvent.input(input, { target: { value: '日本語' } })
    await fireEvent.keyDown(input, { key: 'Enter', isComposing: true })
    await fireEvent.keyDown(input, { key: 'Escape', isComposing: true })
    expect(emitTo).not.toHaveBeenCalled()
    expect(invoke).not.toHaveBeenCalled()
  })

  it('keeps rejected text and ignores stale replies', async () => {
    const input = await open()
    await typeAndSend(input)
    await handlers['launcher-result']({ payload: { id: 'stale', error: '' } })
    expect(invoke).not.toHaveBeenCalled()
    await reply('Finish the current reply and retry.')
    expect(input.value).toBe('  Organize my notes  ')
    expect(screen.getByRole('alert').textContent).toBe('Finish the current reply and retry.')
    await fireEvent.keyDown(input, { key: 'Enter' })
    expect(emitTo).toHaveBeenCalledTimes(2)
  })

  it('keeps text when delivery fails and never resends after acceptance', async () => {
    const input = await open()
    emitTo.mockRejectedValueOnce(new Error('offline'))
    await typeAndSend(input)
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('did not send'))
    expect(input.value).toBe('  Organize my notes  ')
    await fireEvent.keyDown(input, { key: 'Enter' })
    invoke.mockRejectedValueOnce(new Error('window unavailable'))
    await reply()
    await waitFor(() => expect(input.value).toBe(''))
    await fireEvent.keyDown(input, { key: 'Enter' })
    expect(emitTo).toHaveBeenCalledTimes(2)
    expect(screen.getByRole('alert').textContent).toContain('The message sent.')
  })
})

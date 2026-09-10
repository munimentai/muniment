import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Launcher from './Launcher.svelte'

let handlers
let invoke
let emitTo
let stops
beforeEach(() => {
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
afterEach(() => { cleanup(); delete window.__TAURI__ })

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

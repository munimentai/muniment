// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it, vi } from 'vitest'

import ThreadRetention from './ThreadRetention.svelte'

afterEach(cleanup)

function renderRetention(invoke) {
  return render(ThreadRetention, { props: { tauri: { invoke } } })
}

function deferred() {
  let resolve
  let reject
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

describe('thread retention on Data', () => {
  it('renders the default choice selected', async () => {
    renderRetention(vi.fn(async () => null))
    expect(screen.getByText('Choose how long Muniment keeps completed threads.')).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: 'Keep every thread' })).toBeChecked()
    expect(screen.getByRole('radio', { name: 'Delete after 30 days' })).not.toBeChecked()
    expect(screen.getByRole('radio', { name: 'Delete after 90 days' })).not.toBeChecked()
    expect(screen.getByRole('radio', { name: 'Delete after 1 year' })).not.toBeChecked()
  })

  it('records a thread retention choice and restores it on a later read', async () => {
    let recordedChoice = null
    const invoke = vi.fn()
    invoke.mockImplementation(async (command, args) => {
      if (command === 'thread_retention_choice') return recordedChoice
      if (command === 'record_thread_retention_choice') {
        recordedChoice = args.choice
        return
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderRetention(invoke)
    await fireEvent.click(screen.getByRole('radio', { name: 'Delete after 90 days' }))

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_thread_retention_choice', { choice: 'delete_after_90_days' }))

    await waitFor(() => expect(screen.getByRole('radio', { name: 'Delete after 90 days' })).toBeChecked())

    // A later visit to the section reads the saved choice back.
    cleanup()
    renderRetention(invoke)
    await waitFor(() => expect(screen.getByRole('radio', { name: 'Delete after 90 days' })).toBeChecked())
    expect(invoke.mock.calls.filter(([command]) => command === 'thread_retention_choice')).toHaveLength(2)
  })

  it('keeps a save active when the popover reloads before the save resolves', async () => {
    const save = deferred()
    const reload = deferred()
    let loadCalls = 0
    const invoke = vi.fn()
    invoke.mockImplementation(async (command) => {
      if (command === 'thread_retention_choice') {
        loadCalls += 1
        return loadCalls === 1 ? 'keep_every_thread' : reload.promise
      }
      if (command === 'record_thread_retention_choice') return save.promise
      throw new Error(`unexpected command: ${command}`)
    })
    renderRetention(invoke)
    await fireEvent.click(screen.getByRole('radio', { name: 'Delete after 90 days' }))

    expect(screen.getByRole('radio', { name: 'Delete after 90 days' })).toBeDisabled()
    save.resolve()

    await waitFor(() => {
      expect(screen.getByRole('radio', { name: 'Delete after 90 days' })).toBeChecked()
      expect(screen.getByRole('radio', { name: 'Delete after 90 days' })).toBeEnabled()
    })
    reload.resolve(null)
    await reload.promise
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.getByRole('radio', { name: 'Delete after 90 days' })).toBeChecked()
  })

  it('restores the recorded choice and retries a failed save', async () => {
    const firstSave = deferred()
    const invoke = vi.fn()
    invoke.mockImplementation(async (command, args) => {
      if (command === 'thread_retention_choice') return 'keep_every_thread'
      if (command === 'record_thread_retention_choice') {
        if (invoke.mock.calls.filter(([called]) => called === command).length === 1) return firstSave.promise
        expect(args).toEqual({ choice: 'delete_after_30_days' })
        return
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderRetention(invoke)
    await fireEvent.click(screen.getByRole('radio', { name: 'Delete after 30 days' }))
    firstSave.reject(new Error('save failed'))

    expect(await screen.findByText('Thread retention could not be saved.')).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: 'Keep every thread' })).toBeChecked()
    expect(screen.getByRole('radio', { name: 'Delete after 30 days' })).not.toBeChecked()
    await fireEvent.click(screen.getByRole('button', { name: 'Try again' }))

    await waitFor(() => expect(screen.getByRole('radio', { name: 'Delete after 30 days' })).toBeChecked())
    expect(screen.queryByText('Thread retention could not be saved.')).not.toBeInTheDocument()
    expect(invoke.mock.calls.filter(([command]) => command === 'record_thread_retention_choice')).toHaveLength(2)
  })
})

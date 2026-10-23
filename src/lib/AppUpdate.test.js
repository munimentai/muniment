import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, fireEvent, waitFor } from '@testing-library/svelte'
import { afterEach, describe, it, expect, vi } from 'vitest'
import AppUpdate from './AppUpdate.svelte'

afterEach(cleanup)

describe('application update', () => {
  it('stays hidden without a verified download', async () => {
    const invoke = vi.fn().mockResolvedValue(null)
    render(AppUpdate, { tauri: { invoke } })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('app_update_prepare'))
    expect(screen.queryByRole('button')).toBeNull()
  })
  it('installs once, and offers a retry after failure', async () => {
    const invoke = vi.fn(command => command === 'app_update_prepare' ? Promise.resolve('1.2.3') : Promise.reject('The update could not be installed.'))
    render(AppUpdate, { tauri: { invoke } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Install update 1.2.3 and restart' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('could not be installed')
    expect(invoke).toHaveBeenCalledWith('app_update_install')
    expect(screen.getByRole('button')).toBeEnabled()
  })
  it('names the replies an install stops and installs only once confirmed', async () => {
    const invoke = vi.fn(async (command) => ({ app_update_prepare: '1.2.3', app_update_interruptions: 1 })[command] ?? null)
    render(AppUpdate, { tauri: { invoke } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Install update 1.2.3 and restart' }))
    const dialog = await screen.findByRole('dialog', { name: 'Install update 1.2.3?' })
    expect(dialog).toHaveTextContent('Installing now stops 1 running reply. It resumes after the update.')
    expect(invoke).not.toHaveBeenCalledWith('app_update_install')
    await fireEvent.click(screen.getByRole('button', { name: 'Not now' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(invoke).not.toHaveBeenCalledWith('app_update_install')

    invoke.mockImplementation(async (command) => ({ app_update_prepare: '1.2.3', app_update_interruptions: 2 })[command] ?? null)
    await fireEvent.click(screen.getByRole('button', { name: 'Install update 1.2.3 and restart' }))
    expect(await screen.findByRole('dialog')).toHaveTextContent('Installing now stops 2 running replies. They resume after the update.')
    await fireEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('app_update_install'))
  })
  it('installs at once when no reply runs', async () => {
    const invoke = vi.fn(async (command) => ({ app_update_prepare: '1.2.3', app_update_interruptions: 0 })[command] ?? null)
    render(AppUpdate, { tauri: { invoke } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Install update 1.2.3 and restart' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('app_update_install'))
    expect(screen.queryByRole('dialog')).toBeNull()
  })
  it('waits for active work', async () => {
    const invoke = vi.fn().mockResolvedValue('1.2.3')
    render(AppUpdate, { tauri: { invoke }, busy: true })
    expect(await screen.findByRole('button')).toBeDisabled()
    expect(invoke).not.toHaveBeenCalledWith('app_update_install')
  })
  it('shows an update a paired client requested once the app downloads it', async () => {
    const invoke = vi.fn().mockResolvedValue(null)
    let announce
    const listen = vi.fn(async (event, listener) => { if (event === 'app-update-ready') announce = listener; return () => {} })
    render(AppUpdate, { tauri: { invoke }, listen })
    await waitFor(() => expect(announce).toBeTypeOf('function'))
    expect(screen.queryByRole('button')).toBeNull()
    announce({ payload: '0.3.0' })
    expect(await screen.findByRole('button', { name: 'Install update 0.3.0 and restart' })).toBeEnabled()
  })
})

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
  it('waits for active work', async () => {
    const invoke = vi.fn().mockResolvedValue('1.2.3')
    render(AppUpdate, { tauri: { invoke }, busy: true })
    expect(await screen.findByRole('button')).toBeDisabled()
    expect(invoke).not.toHaveBeenCalledWith('app_update_install')
  })
})

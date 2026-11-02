import '@testing-library/jest-dom/vitest'
import { cleanup, render, fireEvent, waitFor } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import ContextSettings from './ContextSettings.svelte'
afterEach(cleanup)
it('loads and saves native compaction preferences', async () => {
  const tauri = { invoke: vi.fn(async () => ({ enabled: true, reserveTokens: 16384, keepRecentTokens: 20000 })) }
  const view = render(ContextSettings, { tauri })
  await fireEvent.click(await view.findByLabelText('Compact automatically'))
  await fireEvent.click(view.getByRole('button', { name: 'Save context settings' }))
  await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('context_settings_save', { enabled: false, reserveTokens: 16384, keepRecentTokens: 20000 }))
  expect(view.getByRole('status')).toHaveTextContent('Applies to the next message')
})
it('keeps Save context settings off while the settings match the saved ones', async () => {
  const tauri = { invoke: vi.fn(async () => ({ enabled: true, reserveTokens: 16384, keepRecentTokens: 20000 })) }
  const view = render(ContextSettings, { tauri })
  const save = await view.findByRole('button', { name: 'Save context settings' })
  expect(save).toBeDisabled()
  const reserve = view.getByLabelText('Room for the next reply (tokens)')
  await fireEvent.input(reserve, { target: { value: '32768' } })
  expect(save).toBeEnabled()
  await fireEvent.input(reserve, { target: { value: '16384' } })
  expect(save).toBeDisabled()
  await fireEvent.click(view.getByLabelText('Compact automatically'))
  expect(save).toBeEnabled()
  await fireEvent.click(save)
  await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('context_settings_save', { enabled: false, reserveTokens: 16384, keepRecentTokens: 20000 }))
  await waitFor(() => expect(save).toBeDisabled())
})

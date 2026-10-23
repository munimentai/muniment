import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import CloudVault from './CloudVault.svelte'

afterEach(cleanup)

function bridge(shareModels) {
  let viaCloud = false
  const list = () => ({ appVersion: '0.2.0', deviceName: 'Laptop', selected: 'local', runtimes: [], shareViaCloud: viaCloud })
  const invoke = vi.fn(async (command, payload) => {
    if (command === 'runtime_list') return list()
    if (command === 'agent_request') return { share_models: shareModels }
    if (command === 'runtime_share_models') { viaCloud = payload.cloud; return list() }
    throw new Error(`unexpected command: ${command}`)
  })
  return { invoke }
}

it('keeps shared model connections in MuniCloud once Machines shares them', async () => {
  const tauri = bridge(true)
  render(CloudVault, { tauri, listen: async () => () => {} })
  const toggle = await screen.findByRole('switch', { name: 'Keep model connections in MuniCloud' })
  await waitFor(() => expect(toggle).toBeEnabled())
  await fireEvent.click(toggle)
  await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('runtime_share_models', { cloud: true }))
  await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'))
})

it('waits for Share model connections on Machines', async () => {
  render(CloudVault, { tauri: bridge(false), listen: async () => () => {} })
  const toggle = await screen.findByRole('switch', { name: 'Keep model connections in MuniCloud' })
  await waitFor(() => expect(screen.getByRole('tooltip', { hidden: true })).toHaveTextContent('Turn on Share model connections in Machines first.'))
  expect(toggle).toBeDisabled()
})

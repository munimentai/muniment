import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelsSection from './ModelsSection.svelte'
afterEach(cleanup)

const inventory = {
  providers: [
    { id: 'custom-omlx', name: 'oMLX', source: 'custom', base_url: 'http://127.0.0.1:8080/v1', models: [{ id: 'Qwen3.8-27B-4bit' }, { id: 'gemma-4-26b-a4b-it-4bit' }] },
    { id: 'custom-empty', name: 'Spare box', source: 'custom', base_url: 'http://10.0.0.2:4000/v1', models: [] },
  ],
  hidden: [], shown: [], default_provider: 'custom-omlx', default_model: 'Qwen3.8-27B-4bit',
}

function bridge() {
  return { invoke: vi.fn(async (command) => {
    if (command === 'local_mode_provider_inventory') return inventory
    if (command === 'model_router_settings') return { accounts: [], subscriptions: [], families: [] }
    if (command === 'fleet_accounts') return null
    return null
  }) }
}

it('names how many models each endpoint serves and what Refresh models found', async () => {
  const tauri = bridge()
  render(ModelsSection, { props: { tauri, listen: async () => () => {}, inventory, sidebarTab: 'accounts' } })
  const omlx = screen.getByRole('region', { name: 'oMLX' })
  expect(within(omlx).getByText('2 models')).toBeInTheDocument()
  expect(within(screen.getByRole('region', { name: 'Spare box' })).getByText('No models')).toBeInTheDocument()
  await fireEvent.click(await screen.findByRole('button', { name: 'Refresh models' }))
  expect(await screen.findByText('Models refreshed. 2 models from 1 provider.')).toBeInTheDocument()
  expect(tauri.invoke).toHaveBeenCalledWith('local_mode_provider_inventory', { force: true })
})

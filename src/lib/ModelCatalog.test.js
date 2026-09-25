import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelCatalog from './ModelCatalog.svelte'
import ModelsSection from './ModelsSection.svelte'

afterEach(cleanup)
const inventory = () => ({ providers: [{ id: 'anthropic', models: [{ id: 'first' }, { id: 'second' }] }], hidden: [], default_provider: 'anthropic', default_model: 'second' })
const switches = () => screen.getAllByRole('switch')

it('updates the toggle immediately without discovery or blocking another model', async () => {
  let finish
  const tauri = { invoke: vi.fn(() => new Promise(resolve => { finish = resolve })) }
  const oninventory = vi.fn()
  render(ModelCatalog, { tauri, inventory: inventory(), oninventory })
  await fireEvent.click(switches()[0])
  expect(switches()[0]).toHaveAttribute('aria-checked', 'false')
  expect(switches()[1]).toBeEnabled()
  finish()
  await waitFor(() => expect(switches()[0]).toBeEnabled())
  expect(tauri.invoke.mock.calls).toEqual([['local_mode_set_model_hidden', { provider: 'anthropic', model: 'first', hidden: true }]])
  expect(oninventory.mock.calls[0][0].hidden).toEqual(['anthropic/first'])
})

it('restores the toggle and shows an error when the write fails', async () => {
  let fail
  const tauri = { invoke: vi.fn(() => new Promise((_, reject) => { fail = reject })) }
  render(ModelCatalog, { tauri, inventory: inventory() })
  await fireEvent.click(switches()[0])
  fail(new Error('Cannot save'))
  await screen.findByRole('alert')
  expect(switches()[0]).toHaveAttribute('aria-checked', 'true')
  expect(switches()[0]).toBeEnabled()
})

it('saves a visible fallback when the selected model is hidden without discovery', async () => {
  const tauri = { invoke: vi.fn(async () => {}) }
  const oninventory = vi.fn()
  render(ModelCatalog, { tauri, inventory: inventory(), oninventory })
  await fireEvent.click(switches()[1])
  await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('local_mode_set_default_model', { provider: 'anthropic', model: 'first' }))
  expect(tauri.invoke).not.toHaveBeenCalledWith('local_mode_provider_inventory')
  expect(oninventory.mock.calls.at(-1)[0].default_model).toBe('first')
})

it('keeps both preferences when separate saves finish out of order', async () => {
  const finish = {}
  const tauri = { invoke: vi.fn((_, payload) => new Promise(resolve => { finish[payload.model] = resolve })) }
  const oninventory = vi.fn()
  render(ModelCatalog, { tauri, inventory: { ...inventory(), default_model: 'unrelated' }, oninventory })
  await fireEvent.click(switches()[0])
  await fireEvent.click(switches()[1])
  finish.second()
  await waitFor(() => expect(switches()[1]).toBeEnabled())
  finish.first()
  await waitFor(() => expect(switches()[0]).toBeEnabled())
  expect(oninventory.mock.calls.at(-1)[0].hidden.sort()).toEqual(['anthropic/first', 'anthropic/second'])
})


it('does not rediscover models when the parent receives a visibility update', async () => {
  const tauri = { invoke: vi.fn(async command => command === 'model_router_settings' ? { accounts: [], options: [] } : inventory()) }
  const view = render(ModelsSection, { tauri, inventory: inventory(), initialTab: 'models' })
  await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh models' })).toBeEnabled())
  expect(tauri.invoke.mock.calls.filter(([command]) => command === 'local_mode_provider_inventory')).toHaveLength(1)
  await view.rerender({ tauri, inventory: { ...inventory(), hidden: ['anthropic/first'] }, initialTab: 'models' })
  await new Promise(resolve => setTimeout(resolve, 20))
  expect(tauri.invoke.mock.calls.filter(([command]) => command === 'local_mode_provider_inventory')).toHaveLength(1)
})


it('combines reported capabilities and context filters without changing saved visibility', async () => {
  const data = inventory()
  data.providers[0].models = [
    { id: 'vision-small', context: '32K', images: true, thinking: false },
    { id: 'vision-large', context: '1M', images: true, thinking: true },
    { id: 'unknown' },
  ]
  const tauri = { invoke: vi.fn() }
  render(ModelCatalog, { tauri, inventory: data })
  await fireEvent.click(screen.getByRole('button', { name: 'Filters' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Vision' }))
  expect(screen.queryByText('Unknown')).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Minimum context: Any size' }))
  await fireEvent.click(screen.getByRole('menuitemradio', { name: '128K+' }))
  expect(screen.getAllByRole('switch')).toHaveLength(1)
  await fireEvent.click(screen.getByRole('button', { name: 'Reasoning' }))
  expect(screen.getAllByRole('switch')).toHaveLength(1)
  await fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
  expect(screen.getAllByRole('switch')).toHaveLength(3)
  expect(tauri.invoke).not.toHaveBeenCalled()
})

it('selects either visibility segment with pointer or keyboard without changing preferences', async () => {
  const tauri = { invoke: vi.fn() }
  render(ModelCatalog, { tauri, inventory: { ...inventory(), hidden: ['anthropic/first'] } })
  await fireEvent.click(screen.getByRole('button', { name: 'Filters' }))
  const enabled = screen.getByRole('radio', { name: 'Enabled' })
  await fireEvent.click(enabled)
  expect(enabled).toHaveAttribute('aria-checked', 'true')
  expect(switches()).toHaveLength(1)
  await fireEvent.keyDown(enabled, { key: 'ArrowLeft' })
  expect(screen.getByRole('radio', { name: 'All models' })).toHaveAttribute('aria-checked', 'true')
  expect(switches()).toHaveLength(2)
  expect(tauri.invoke).not.toHaveBeenCalled()
})

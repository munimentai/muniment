import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelsSection from './ModelsSection.svelte'
import ClassifierConnection from './ClassifierConnection.svelte'
import { CLASSIFIERS } from './classifier-connections.js'
import { catalogProvider, pickerGroups } from './provider-catalog.js'
afterEach(cleanup)
const inventory = { providers: [], hidden: [] }
const settings = { accounts: [], classifier_connections: [], options: [] }
it('places classifiers between popular accounts and the Pi catalog', async () => {
  const tauri = { invoke: vi.fn(async command => command === 'model_router_settings' ? settings : inventory) }
  render(ModelsSection, { tauri, inventory })
  await fireEvent.click(await screen.findByRole('button', { name: 'Connect account' }))
  const sections = screen.getAllByRole('heading', { level: 5 }).map(el => el.textContent)
  expect(sections).toEqual(['Popular & Subscriptions', 'Classifiers', 'All Pi providers'])
  expect(CLASSIFIERS[0].name).toBe('Jev')
  expect(CLASSIFIERS).toHaveLength(10)
  await fireEvent.click(screen.getByRole('button', { name: /Jev Hosted API/ }))
  expect(screen.getByRole('heading', { name: 'Connect Jev' })).toBeVisible()
  expect(screen.getByRole('button', { name: 'TypeSafe API' })).toBeVisible()
  expect(screen.getByRole('button', { name: 'OpenRouter API' })).toBeVisible()
  expect(screen.getByRole('button', { name: 'Cloudflare Workers AI' })).toBeVisible()
})
it('tests a connection before saving and stays on failures', async () => {
  const connected = vi.fn()
  const tauri = { invoke: vi.fn().mockRejectedValueOnce(new Error('The classifier refused the key.')).mockResolvedValueOnce(settings) }
  render(ClassifierConnection, { entry: CLASSIFIERS[0], tauri, onconnected: connected })
  await fireEvent.input(screen.getByLabelText('API key'), { target: { value: 'test-key' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Test and connect' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('The classifier refused the key.')
  expect(connected).not.toHaveBeenCalled()
  await fireEvent.click(screen.getByRole('button', { name: 'Test and connect' }))
  await waitFor(() => expect(connected).toHaveBeenCalledWith(settings))
  expect(tauri.invoke).toHaveBeenCalledWith('model_router_connect_classifier', expect.objectContaining({ catalogId: 'jev', baseUrl: 'https://api.typesafe.ai/v1/systemone', apiKey: 'test-key' }))
})
it('builds a Cloudflare connection from its account ID and token', async () => {
  const tauri = { invoke: vi.fn().mockResolvedValue(settings) }
  render(ClassifierConnection, { entry: CLASSIFIERS[0], tauri, onconnected: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: 'Cloudflare Workers AI' }))
  await fireEvent.input(screen.getByLabelText('Cloudflare account ID'), { target: { value: 'a'.repeat(32) } })
  await fireEvent.input(screen.getByLabelText('API key'), { target: { value: 'token' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Test and connect' }))
  expect(tauri.invoke).toHaveBeenCalledWith('model_router_connect_classifier', expect.objectContaining({ baseUrl: `https://api.cloudflare.com/client/v4/accounts/${'a'.repeat(32)}/ai/run`, model: 'typesafe/jev' }))
})
it('explains downloads that require an adapter', async () => {
  render(ClassifierConnection, { entry: CLASSIFIERS.find(entry => entry.id === 'needle'), tauri: { invoke: vi.fn() } })
  await fireEvent.click(screen.getByRole('button', { name: 'Download and run locally' }))
  expect(screen.getByText(/Downloading its weights alone does not connect it/)).toBeVisible()
  expect(screen.queryByRole('button', { name: 'Test and connect' })).toBeNull()
})
it('combines Kimi methods and keeps classifier connections outside the chat catalog', () => {
  expect(catalogProvider('kimi')).toMatchObject({ keyProvider: 'kimi-coding', methods: ['account', 'key'] })
  expect(catalogProvider('kimi-coding')).toBeNull()
  expect(catalogProvider('vllm').baseUrl).toBe('http://localhost:8000/v1')
  expect(pickerGroups({ ...inventory, classifier_connections: [{ id: 'jev' }] })).toEqual([])
})

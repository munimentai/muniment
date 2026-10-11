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
it('places classifiers between popular accounts and the full provider catalog', async () => {
  const tauri = { invoke: vi.fn(async command => command === 'model_router_settings' ? settings : inventory) }
  render(ModelsSection, { tauri, inventory })
  await fireEvent.click(await screen.findByRole('button', { name: 'Connect account' }))
  const sections = screen.getAllByRole('heading', { level: 5 }).map(el => el.textContent)
  expect(sections).toEqual(['Popular & Subscriptions', 'Decision models', 'All providers'])
  expect(CLASSIFIERS[0].name).toBe('Jev')
  expect(CLASSIFIERS.map(entry => entry.name)).toEqual(['Jev', 'Clef', 'GPT-6 Luna', 'Kev', 'Nimble', 'Von', 'Laya', 'Decider 4B'])
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
it('shows the setup a local server needs before it connects', async () => {
  render(ClassifierConnection, { entry: CLASSIFIERS.find(entry => entry.id === 'decider'), tauri: { invoke: vi.fn() } })
  await fireEvent.click(screen.getByRole('button', { name: 'Download and run locally' }))
  expect(screen.getByText(/scripts\/serve.sh Mapika\/decider-4b 8000/)).toBeVisible()
  expect(screen.queryByRole('button', { name: 'Test and connect' })).toBeNull()
})
it('connects Clef Flash or Clef by the id each way to connect uses', async () => {
  const tauri = { invoke: vi.fn().mockResolvedValue(settings) }
  const ollama = { id: 'ollama', base_url: 'https://ollama.example/v1' }
  render(ClassifierConnection, { entry: CLASSIFIERS.find(entry => entry.id === 'clef'), tauri, ollama, onconnected: vi.fn() })
  expect(screen.getByLabelText('Model ID')).toHaveValue('@cf/cloudflare/clef-flash')
  await fireEvent.click(screen.getByRole('button', { name: 'Clef' }))
  expect(screen.getByLabelText('Model ID')).toHaveValue('@cf/cloudflare/clef')
  await fireEvent.click(screen.getByRole('button', { name: 'OpenRouter API' }))
  expect(screen.getByLabelText('Model ID')).toHaveValue('cloudflare/clef')
  // The Ollama server's own System One route, with the key the server holds.
  await fireEvent.click(screen.getByRole('button', { name: 'Ollama server' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Clef Flash' }))
  expect(screen.getByLabelText('Decision model URL')).toHaveValue('https://ollama.example/v1/systemone')
  expect(screen.queryByLabelText(/API key/)).toBeNull()
  await fireEvent.click(screen.getByRole('button', { name: 'Test and connect' }))
  expect(tauri.invoke).toHaveBeenCalledWith('model_router_connect_classifier', { catalogId: 'clef', name: 'Clef Flash · Ollama server', model: 'clef-flash', baseUrl: 'https://ollama.example/v1/systemone', apiKey: null, keyProvider: 'ollama' })
})
it('combines Kimi methods and keeps classifier connections outside the chat catalog', () => {
  expect(catalogProvider('kimi')).toMatchObject({ keyProvider: 'kimi-coding', methods: ['account', 'key'] })
  expect(catalogProvider('kimi-coding')).toBeNull()
  expect(catalogProvider('vllm').baseUrl).toBe('http://localhost:8000/v1')
  expect(pickerGroups({ ...inventory, classifier_connections: [{ id: 'jev' }] })).toEqual([])
})
it('lists each connected classifier with its logo and edits it in place, keeping a blank key', async () => {
  const connection = { id: 'c1', name: 'Kev local', catalog_id: 'kev', active: true, connection: { kind: 'endpoint', model: 'kev-latest', family: '', base_url: 'http://127.0.0.1:8009/v1/systemone', configured: true } }
  const connected = { ...settings, classifier_connections: [connection] }
  const renamed = { ...connected, classifier_connections: [{ ...connection, name: 'Kev 4B', connection: { ...connection.connection, model: 'kev-4b' } }] }
  const tauri = { invoke: vi.fn(async command => command === 'model_router_update_classifier' ? renamed : command === 'model_router_settings' ? connected : inventory) }
  render(ModelsSection, { tauri, inventory, initialTab: 'accounts' })
  const row = (await screen.findByText('Kev local')).closest('.account-row')
  expect(row.querySelector('.provider-logo, svg, img')).not.toBeNull()
  // The row's actions are icons with names, and no words.
  expect(screen.getByRole('button', { name: 'Edit Kev local' }).querySelector('[data-icon="square-pen"]')).not.toBeNull()
  expect(screen.getByRole('button', { name: 'Disconnect Kev local' }).querySelector('[data-icon="unplug"]')).not.toBeNull()
  expect(row).not.toHaveTextContent(/Edit|Disconnect/)
  await fireEvent.click(screen.getByRole('button', { name: 'Edit Kev local' }))
  expect(screen.getByLabelText('Decision model URL')).toHaveValue('http://127.0.0.1:8009/v1/systemone')
  expect(screen.getByLabelText('API key')).toHaveValue('')
  await fireEvent.input(screen.getByLabelText('Connection name'), { target: { value: 'Kev 4B' } })
  await fireEvent.input(screen.getByLabelText('Model ID'), { target: { value: 'kev-4b' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  expect(tauri.invoke).toHaveBeenCalledWith('model_router_update_classifier', { id: 'c1', name: 'Kev 4B', model: 'kev-4b', baseUrl: 'http://127.0.0.1:8009/v1/systemone', apiKey: null })
  expect(await screen.findByText('Kev 4B')).toBeVisible()
  expect(screen.queryByRole('form', { name: 'Edit Kev local' })).toBeNull()
})
it('edits a connected Ollama server through its own form, keeping the saved key when the field stays blank', async () => {
  const ollama = { providers: [{ id: 'ollama', name: 'Ollama', source: 'local', base_url: 'https://ollama.example/v1', models: [{ id: 'nimble:latest' }] }], hidden: [] }
  const tauri = { invoke: vi.fn(async command => command === 'model_router_settings' ? settings : ollama) }
  render(ModelsSection, { tauri, inventory: ollama, initialTab: 'accounts' })
  await fireEvent.click(await screen.findByRole('button', { name: 'Edit Ollama' }))
  expect(screen.getByRole('heading', { name: 'Edit Ollama' })).toBeVisible()
  expect(screen.getByLabelText('Ollama server URL')).toHaveValue('https://ollama.example/v1')
  expect(screen.getByLabelText('API key (optional)')).toHaveAttribute('placeholder', 'Leave blank to keep the saved key')
  await fireEvent.click(screen.getByRole('button', { name: 'Save Ollama server' }))
  expect(tauri.invoke).toHaveBeenCalledWith('local_mode_store_local_provider', { baseUrl: 'https://ollama.example/v1', apiKey: null })
})
it('saves a Cloudflare key with the account it belongs to', async () => {
  const tauri = { invoke: vi.fn(async command => command === 'model_router_settings' ? settings : inventory) }
  render(ModelsSection, { tauri, inventory })
  await fireEvent.click(await screen.findByRole('button', { name: 'Connect account' }))
  await fireEvent.click(screen.getByRole('button', { name: /^Cloudflare Workers AI/ }))
  await fireEvent.input(screen.getByLabelText('Cloudflare Workers AI API key'), { target: { value: 'cf-token' } })
  expect(screen.getByRole('button', { name: 'Save key' })).toBeDisabled()
  await fireEvent.input(screen.getByLabelText('Cloudflare account ID'), { target: { value: ' abc123 ' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save key' }))
  expect(tauri.invoke).toHaveBeenCalledWith('local_mode_store_provider_key', { provider: 'cloudflare-workers-ai', key: 'cf-token', env: { CLOUDFLARE_ACCOUNT_ID: 'abc123' } })
})
it('connects GPT-6 Luna to the Decisions API with an OpenAI API key', async () => {
  const tauri = { invoke: vi.fn().mockResolvedValue(settings) }
  render(ClassifierConnection, { entry: CLASSIFIERS.find(entry => entry.id === 'luna'), tauri, onconnected: vi.fn() })
  expect(screen.getByRole('button', { name: 'OpenAI API' })).toHaveAttribute('aria-pressed', 'true')
  expect(screen.getByRole('button', { name: 'Test and connect' })).toBeDisabled()
  await fireEvent.input(screen.getByLabelText('API key'), { target: { value: 'openai-key' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Test and connect' }))
  await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('model_router_connect_classifier', expect.objectContaining({ catalogId: 'luna', model: 'gpt-6-luna', baseUrl: 'https://api.openai.com/v1/decisions', apiKey: 'openai-key' })))
})

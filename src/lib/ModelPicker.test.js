import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelPicker from './ModelPicker.svelte'
afterEach(cleanup)

const props = (inventory, extra = {}) => ({ inventory, onchoose: vi.fn(), onmanage: vi.fn(), onclose: vi.fn(), ...extra })

it('shows a column of full-color provider marks named on hover without connection labels', async () => {
  const {container} = render(ModelPicker, props({providers:[
    {id:'xai',name:'xAI Account',source:'account',models:[{id:'grok'}]},
    {id:'claude-bridge',name:'Anthropic Claude Code',source:'claude-code',models:[{id:'claude'}]},
    {id:'router',name:'Router',source:'router',models:[]},
  ],router_classifier:'jev-latest',router_models:[{id:'openai/gpt',family:'openai',model:'gpt',accounts:2}]}))
  const rail = screen.getByRole('tablist', { name: 'Providers' })
  expect(within(rail).getAllByRole('tab').map((tab) => tab.getAttribute('aria-label'))).toEqual(['Model router', 'xAI', 'Anthropic', 'OpenAI'])
  expect(screen.getByRole('tab', { name: 'Model router' }).querySelector('[data-icon=route]')).not.toBeNull()
  // Anthropic's mark is its brand's colored one, as on the model Settings pages.
  expect(screen.getByRole('tab', { name: 'Anthropic' }).querySelector('.logo svg')).not.toBeNull()
  expect(container.querySelector('.rail-tip')).toBeNull()
  await fireEvent.pointerEnter(screen.getByRole('tab', { name: 'Anthropic' }))
  expect(container.querySelector('.rail-tip')).toHaveTextContent('Anthropic')
  await fireEvent.pointerLeave(screen.getByRole('tab', { name: 'Anthropic' }))
  expect(container.querySelector('.rail-tip')).toBeNull()

  // The first provider opens, and a mark opens its own models.
  expect(screen.getByRole('tab', { name: 'Model router' })).toHaveAttribute('aria-selected', 'true')
  expect(screen.getByRole('button', { name: 'Jev Latest picks' }).querySelector('.logo svg')).not.toBeNull()
  await fireEvent.click(screen.getByRole('tab', { name: 'OpenAI' }))
  expect(screen.getByRole('tabpanel', { name: 'OpenAI' })).toBeInTheDocument()
  expect(container.querySelector('.balanced [data-icon=scale]')).not.toBeNull()
  expect(container.textContent).not.toContain('Claude Code')
})

it('opens on the provider of the model in use and folds older generations under Legacy models', async () => {
  const inventory = { providers: [
    { id: 'openai', name: 'OpenAI', source: 'key', models: [{ id: 'gpt-6.1-sol' }] },
    { id: 'anthropic', name: 'Anthropic', source: 'key', models: [{ id: 'claude-opus-5-5' }, { id: 'claude-opus-4-8' }, { id: 'claude-sonnet-5-5' }, { id: 'claude-sonnet-4-6' }] },
  ], hidden: [], shown: [], default_provider: 'anthropic', default_model: 'claude-opus-5-5' }
  const onchoose = vi.fn()
  render(ModelPicker, props(inventory, { onchoose, current: { provider: 'anthropic', model: 'claude-opus-5-5' } }))
  expect(screen.getByRole('tab', { name: 'Anthropic' })).toHaveAttribute('aria-selected', 'true')
  const panel = screen.getByRole('tabpanel', { name: 'Anthropic' })
  expect([...panel.querySelectorAll('.picker-row')].map((row) => row.dataset.model)).toEqual(['claude-sonnet-5-5', 'claude-opus-5-5'])
  const legacy = within(panel).getByRole('button', { name: /Legacy models/ })
  expect(legacy).toHaveTextContent('2 models')
  expect(legacy).toHaveAttribute('aria-expanded', 'false')
  await fireEvent.click(legacy)
  expect(legacy).toHaveAttribute('aria-expanded', 'true')
  const older = within(panel).getByRole('list', { name: 'Legacy models' })
  expect([...older.querySelectorAll('.picker-row')].map((row) => row.dataset.model)).toEqual(['claude-opus-4-8', 'claude-sonnet-4-6'])
  await fireEvent.click(within(older).getByRole('button', { name: 'Claude Opus 4.8' }))
  expect(onchoose).toHaveBeenCalledWith('anthropic', 'claude-opus-4-8')
})

it('keeps an older saved model in the current list and searches every provider', async () => {
  const inventory = { providers: [
    { id: 'openai', name: 'OpenAI', source: 'key', models: [{ id: 'gpt-6-luna' }, { id: 'gpt-5.6-luna' }, { id: 'gpt-5.5-luna' }] },
    { id: 'anthropic', name: 'Anthropic', source: 'key', models: [{ id: 'claude-opus-5-5' }, { id: 'claude-opus-4-8' }] },
  ], hidden: [], shown: [], default_provider: 'openai', default_model: 'gpt-5.6-luna' }
  const { container } = render(ModelPicker, props(inventory, { current: { provider: 'openai', model: 'gpt-5.6-luna' } }))
  expect([...container.querySelectorAll('.picker-row')].map((row) => row.dataset.model)).toEqual(['gpt-6-luna', 'gpt-5.6-luna'])
  expect(screen.getByRole('button', { name: 'GPT 5.6 Luna' })).toHaveAttribute('aria-pressed', 'true')
  expect(screen.getByRole('button', { name: /Legacy models/ })).toHaveTextContent('1 model')
  await fireEvent.input(screen.getByRole('searchbox', { name: 'Search models' }), { target: { value: '4-8' } })
  expect(screen.getByRole('tabpanel', { name: 'Search results' })).toHaveTextContent('Claude Opus 4.8')
  expect(screen.getAllByRole('tab').every((tab) => tab.getAttribute('aria-selected') === 'false')).toBe(true)
  await fireEvent.click(screen.getByRole('tab', { name: 'Anthropic' }))
  expect(screen.getByRole('searchbox', { name: 'Search models' })).toHaveValue('')
  expect(screen.getByRole('tabpanel', { name: 'Anthropic' })).toHaveTextContent('Claude Opus 5.5')
})

it('moves a model between its current list and Legacy models', async () => {
  const inventory = { providers: [{ id: 'anthropic', name: 'Anthropic', source: 'key', models: [{ id: 'claude-opus-5-5' }, { id: 'claude-opus-4-8' }] }], hidden: [], shown: [] }
  const onvisibility = vi.fn()
  render(ModelPicker, props(inventory, { onvisibility }))
  await fireEvent.click(screen.getByRole('button', { name: 'Hide Claude Opus 5.5' }))
  expect(onvisibility).toHaveBeenCalledWith(expect.objectContaining({ provider: 'anthropic', choice: 'claude-opus-5-5' }), false)
  await fireEvent.click(screen.getByRole('button', { name: /Legacy models/ }))
  await fireEvent.click(screen.getByRole('button', { name: 'Show Claude Opus 4.8' }))
  expect(onvisibility).toHaveBeenCalledWith(expect.objectContaining({ provider: 'anthropic', choice: 'claude-opus-4-8' }), true)
})

it('shows no context window and marks a model the conversation compacts on', () => {
  const usage = { tokens: 46210, contextWindow: 200000, percent: 23.1, estimated: false, windowKnown: true, autoCompact: true, compactAt: 183616 }
  const {container} = render(ModelPicker, props({providers:[
    {id:'ollama',name:'Ollama',source:'local',models:[{id:'qwen3:8b',context:'40K'},{id:'llama3.2:3b',context:'200K'}]},
  ]}, {contextUsage:usage,current:{provider:'ollama',model:'llama3.2:3b'}}))
  const rows = [...container.querySelectorAll('.picker-row')]
  const llama = rows.find(row => row.dataset.model === 'llama3.2:3b')
  const qwen = rows.find(row => row.dataset.model === 'qwen3:8b')
  expect(container.textContent).not.toMatch(/200K|40K/)
  expect(llama.querySelector('.will-compact')).toBeNull()
  expect(qwen.querySelector('.will-compact')).toHaveTextContent('will compact')
  expect(qwen.querySelector('.will-compact [data-icon=combine]')).not.toBeNull()
})

it('links the Model router heading to Decision models while routing is off', async () => {
  const onrouting = vi.fn()
  render(ModelPicker, props({ providers: [{ id: 'muniment-router', name: 'Router', source: 'router', models: [] }], router_classifier: null, router_models: [{ id: 'openai/gpt', family: 'openai', model: 'gpt', accounts: 2 }] }, { onrouting }))
  expect(screen.getByRole('heading', { name: 'Model router', exact: true })).toBeInTheDocument()
  expect(screen.queryByText('Automatic')).toBeNull()
  await fireEvent.click(screen.getByRole('button', { name: 'Manage model routing' }))
  expect(onrouting).toHaveBeenCalledTimes(1)
})

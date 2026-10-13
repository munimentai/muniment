import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelPicker from './ModelPicker.svelte'
afterEach(cleanup)
it('shows provider logos and names without connection labels', () => {
  const {container} = render(ModelPicker, {inventory:{providers:[
    {id:'xai',name:'xAI Account',source:'account',models:[{id:'grok'}]},
    {id:'claude-bridge',name:'Anthropic Claude Code',source:'claude-code',models:[{id:'claude'}]},
    {id:'router',name:'Router',source:'router',models:[]},
  ],router_classifier:'jev-latest',router_models:[{id:'openai/gpt',family:'openai',model:'gpt',accounts:2}]},onchoose:vi.fn(),onmanage:vi.fn(),onclose:vi.fn()})
  for (const name of ['xAI','Anthropic','OpenAI']) {
    const heading = screen.getByRole('heading',{name,exact:true})
    expect(heading.querySelector('.logo svg')).not.toBeNull()
    expect(heading.querySelector('.blank')).toBeNull()
  }
  expect(screen.getByRole('heading',{name:'Model router',exact:true}).querySelector('[data-icon=route]')).not.toBeNull()
  expect(screen.getByRole('button',{name:'Jev Latest picks'}).querySelector('.logo svg')).not.toBeNull()
  expect(container.querySelector('.balanced [data-icon=scale]')).not.toBeNull()
  expect(container.textContent).not.toContain('Claude Code')
})
it('shows each window and marks a model the conversation compacts on', () => {
  const usage = { tokens: 46210, contextWindow: 200000, percent: 23.1, estimated: false, windowKnown: true, autoCompact: true, compactAt: 183616 }
  const {container} = render(ModelPicker, {inventory:{providers:[
    {id:'anthropic',name:'Anthropic',source:'key',models:[{id:'claude-sonnet',context:'200K'}]},
    {id:'ollama',name:'Ollama',source:'local',models:[{id:'qwen3:8b',context:'40K'}]},
  ]},contextUsage:usage,current:{provider:'anthropic',model:'claude-sonnet'},onchoose:vi.fn(),onmanage:vi.fn(),onclose:vi.fn()})
  const rows = [...container.querySelectorAll('.picker-row')]
  const claude = rows.find(row => row.dataset.model === 'claude-sonnet')
  const qwen = rows.find(row => row.dataset.model === 'qwen3:8b')
  expect(claude.querySelector('.window')).toHaveTextContent('200K')
  expect(claude.querySelector('.will-compact')).toBeNull()
  expect(qwen.querySelector('.window')).toHaveTextContent('40K')
  expect(qwen.querySelector('.will-compact')).toHaveTextContent('will compact')
  expect(qwen.querySelector('.will-compact [data-icon=combine]')).not.toBeNull()
})

it('links the Model router heading to Decision models while routing is off', async () => {
  const { fireEvent } = await import('@testing-library/svelte')
  const onrouting = vi.fn()
  render(ModelPicker, { inventory: { providers: [{ id: 'muniment-router', name: 'Router', source: 'router', models: [] }], router_classifier: null, router_models: [{ id: 'openai/gpt', family: 'openai', model: 'gpt', accounts: 2 }] }, onchoose: vi.fn(), onmanage: vi.fn(), onrouting, onclose: vi.fn() })
  expect(screen.getByRole('heading', { name: 'Model router', exact: true })).toBeInTheDocument()
  expect(screen.queryByText('Automatic')).toBeNull()
  await fireEvent.click(screen.getByRole('button', { name: 'Manage model routing' }))
  expect(onrouting).toHaveBeenCalledTimes(1)
})

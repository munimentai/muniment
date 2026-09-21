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

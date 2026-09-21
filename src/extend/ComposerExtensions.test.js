import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import ComposerExtensions from './ComposerExtensions.svelte'
afterEach(cleanup)
it('persists per-chat MCP toggles and preserves file attachments', async () => {
  const state={items:[{id:'docs',kind:'mcp',name:'Docs',enabled:true}],threads:{chat:{disabled:[],selected:[]}}}
  const invoke=vi.fn(async()=>state), onattach=vi.fn()
  render(ComposerExtensions,{tauri:{invoke},threadId:'chat',onattach,onmanage:vi.fn()})
  await fireEvent.click(screen.getByRole('button',{name:'Tools and attachments'}))
  const toggle=await screen.findByRole('checkbox',{name:'Docs'})
  await fireEvent.click(toggle)
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith('extend_command',{action:'thread',data:{threadId:'chat',selected:[],disabled:['docs'],automatic:false}}))
  await fireEvent.click(screen.getByRole('button',{name:'Add files'}))
  expect(onattach).toHaveBeenCalledOnce()
})
it('invokes a skill without treating its command as a chat message', async () => {
  const invoke=vi.fn(async()=>({items:[{id:'review',kind:'skill',skills:[{name:'review',description:'Review code',path:'SKILL.md'}]}],threads:{}}))
  const {component}=render(ComposerExtensions,{tauri:{invoke},threadId:'chat',draft:'\\review',onattach:vi.fn(),onmanage:vi.fn()})
  await screen.findByRole('button',{name:/review.*skill.*Review code/})
  const event={key:'Enter',preventDefault:vi.fn()}
  expect(component.handleKey(event)).toBe(true)
  await screen.findByRole('button',{name:'Remove review'})
  expect(event.preventDefault).toHaveBeenCalledOnce()
})

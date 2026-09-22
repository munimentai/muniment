import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import ComposerExtensions from './ComposerExtensions.svelte'
import { openComposerPanel } from '../lib/composer-panels.js'
afterEach(cleanup)
it('keeps MCP switches local to one turn and resets after submission', async () => {
  const state={items:[{id:'docs',kind:'mcp',name:'Docs',enabled:true}],threads:{chat:{selected:['docs']}}}
  const invoke=vi.fn(async()=>state)
  const {component}=render(ComposerExtensions,{tauri:{invoke},threadId:'chat',onmanage:vi.fn()})
  await fireEvent.click(screen.getByRole('button',{name:'Extensions'}))
  await fireEvent.click(screen.getByRole('button',{name:'MCPs'}))
  const toggle=await screen.findByRole('switch',{name:'Use Docs for this turn'})
  expect(toggle).not.toBeChecked()
  await fireEvent.click(toggle)
  expect(invoke.mock.calls.some(([,arg])=>arg.action==='turn')).toBe(false)
  await component.prepare('Read docs')
  expect(invoke).toHaveBeenCalledWith('extend_command',{action:'turn',data:{threadId:'chat',selected:['docs'],disabled:[],automatic:false}})
  component.submitted()
  await component.prepare('Next turn')
  expect(invoke).toHaveBeenLastCalledWith('extend_command',{action:'turn',data:{threadId:'chat',selected:[],disabled:[],automatic:false}})
})
it('inserts a slash command into the draft without chips or persisted selection', async () => {
  const invoke=vi.fn(async()=>({items:[{id:'review',kind:'skill',skills:[{name:'review',description:'Review code',path:'SKILL.md'}]}]}))
  const {component}=render(ComposerExtensions,{tauri:{invoke},threadId:'chat',draft:'\\review',onmanage:vi.fn()})
  await screen.findByRole('button',{name:/\/review.*skill.*Review code/})
  const event={key:'Enter',preventDefault:vi.fn()}
  expect(component.handleKey(event)).toBe(true)
  expect(event.preventDefault).toHaveBeenCalledOnce()
  await component.prepare('/review Fix this')
  expect(invoke).toHaveBeenLastCalledWith('extend_command',{action:'turn',data:{threadId:'chat',selected:['review:SKILL.md'],disabled:[],automatic:false}})
  expect(screen.queryByLabelText('Selected extensions')).not.toBeInTheDocument()
  await component.prepare('Fix this')
  expect(invoke).toHaveBeenLastCalledWith('extend_command',{action:'turn',data:{threadId:'chat',selected:[],disabled:[],automatic:false}})
})
it('shares the composer panel coordinator and has three extension branches', async () => {
  render(ComposerExtensions,{tauri:{invoke:vi.fn(async()=>({items:[]}))},threadId:'chat',onmanage:vi.fn()})
  await fireEvent.click(screen.getByRole('button',{name:'Extensions'}))
  expect(screen.getByRole('dialog',{name:'Extensions for this turn'})).toHaveAttribute('data-composer-panel')
  for(const name of ['MCPs','Plugins','Skills']) expect(screen.getByRole('button',{name})).toBeInTheDocument()
  openComposerPanel('model')
  await waitFor(()=>expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
})

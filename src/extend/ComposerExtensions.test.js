import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import ComposerExtensions from './ComposerExtensions.svelte'
import { openComposerPanel } from '../lib/composer-panels.js'
afterEach(cleanup)
it('keeps MCP switches local to one turn and resets after submission', async () => {
  const state={items:[{id:'docs',kind:'mcp',name:'Docs',enabled:true}],threads:{chat:{selected:['docs']}},assist:{name:'Clef Flash'}}
  // The window still shows an older thread. The turn goes to the runtime's current one.
  const invoke=vi.fn(async command=>command==='chat_current_thread'?'chat':state)
  const {component}=render(ComposerExtensions,{tauri:{invoke},threadId:'older',onmanage:vi.fn()})
  await fireEvent.click(screen.getByRole('button',{name:'Extensions'}))
  await fireEvent.click(screen.getByRole('button',{name:'MCPs'}))
  const toggle=await screen.findByRole('switch',{name:'Use Docs for this turn'})
  expect(toggle).not.toBeChecked()
  await fireEvent.click(toggle)
  expect(invoke.mock.calls.some(([,arg])=>arg.action==='turn')).toBe(false)
  // Assistance in Settings turns the decision model on until the user turns it off.
  expect(screen.getByRole('switch',{name:'Clef Flash assists'})).toBeChecked()
  await fireEvent.click(screen.getByRole('switch',{name:'Clef Flash assists'}))
  await component.prepare('Read docs')
  expect(invoke).toHaveBeenCalledWith('extend_command',{action:'turn',data:{threadId:'chat',selected:['docs'],disabled:[],automatic:false}})
  expect(invoke.mock.calls.some(([,arg])=>arg?.action==='route')).toBe(false)
  component.submitted()
  await component.prepare('Next turn')
  expect(invoke).toHaveBeenCalledWith('extend_command',{action:'turn',data:{threadId:'chat',selected:[],disabled:[],automatic:true}})
  expect(invoke).toHaveBeenLastCalledWith('extend_command',{action:'route',data:{threadId:'chat',prompt:'Next turn'}})
})
it('inserts a slash command into the draft without chips or persisted selection', async () => {
  const invoke=vi.fn(async command=>command==='chat_current_thread'?'chat':({items:[{id:'review',kind:'skill',skills:[{name:'review',description:'Review code',path:'SKILL.md'}]}]}))
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
it('opens one branch at a time on hover after a short delay', async () => {
  vi.useFakeTimers()
  try {
    render(ComposerExtensions,{tauri:{invoke:vi.fn(async()=>({items:[]}))},threadId:'chat',onmanage:vi.fn()})
    await fireEvent.click(screen.getByRole('button',{name:'Extensions'}))
    const [mcps, plugins, skills] = ['MCPs','Plugins','Skills'].map(name => screen.getByRole('button',{name}))
    await fireEvent.pointerEnter(mcps)
    expect(mcps).toHaveAttribute('aria-expanded','false')
    await vi.advanceTimersByTimeAsync(200)
    expect(mcps).toHaveAttribute('aria-expanded','true')
    // Crossing a row on the way to the submenu leaves the open branch alone.
    await fireEvent.pointerEnter(plugins)
    await fireEvent.pointerLeave(plugins)
    await vi.advanceTimersByTimeAsync(200)
    expect(mcps).toHaveAttribute('aria-expanded','true')
    await fireEvent.pointerEnter(skills)
    await vi.advanceTimersByTimeAsync(200)
    expect(skills).toHaveAttribute('aria-expanded','true')
    expect(mcps).toHaveAttribute('aria-expanded','false')
    expect(screen.getAllByLabelText(/^Available /)).toHaveLength(1)
  } finally { vi.useRealTimers() }
})
it('hides assistance and never routes while no decision model assists', async () => {
  const invoke=vi.fn(async command=>command==='chat_current_thread'?'chat':{items:[{id:'docs',kind:'mcp',name:'Docs',enabled:true}]})
  const {component}=render(ComposerExtensions,{tauri:{invoke},threadId:'chat',onmanage:vi.fn()})
  await fireEvent.click(screen.getByRole('button',{name:'Extensions'}))
  await screen.findByRole('button',{name:'MCPs'})
  expect(screen.queryByText(/assists$/)).toBeNull()
  await component.prepare('Read docs')
  expect(invoke).toHaveBeenCalledWith('extend_command',{action:'turn',data:{threadId:'chat',selected:[],disabled:[],automatic:false}})
  expect(invoke.mock.calls.some(([,arg])=>arg?.action==='route')).toBe(false)
})

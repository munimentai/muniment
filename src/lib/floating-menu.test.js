import { afterEach, expect, it, vi } from 'vitest'
import { waitFor } from '@testing-library/svelte'
import { Window } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { floatingMenu } from './floating-menu.js'
vi.mock('@tauri-apps/api/window', () => ({ Window: {getByLabel:vi.fn()}, getCurrentWindow: () => ({innerPosition:async()=>({x:0,y:0}),scaleFactor:async()=>2}) }))
vi.mock('@tauri-apps/api/event', () => ({listen:vi.fn().mockResolvedValue(()=>{})}))
afterEach(()=>{delete window.__TAURI_INTERNALS__;delete document.documentElement.dataset.pointer;document.body.replaceChildren();vi.clearAllMocks()})
it('routes popup selections to the original menu and ignores other menu sessions',async()=>{
 window.__TAURI_INTERNALS__={}
 const popup={setSize:vi.fn(),setPosition:vi.fn(),emit:vi.fn(),hide:vi.fn()}
 Window.getByLabel.mockResolvedValue(popup)
 const node=document.createElement('div');node.setAttribute('role','menu');node.innerHTML='<button>Rename</button><button disabled data-danger>Delete</button>';document.body.append(node)
 const click=vi.fn();node.firstChild.addEventListener('click',click)
 const action=floatingMenu(node)
 await waitFor(()=>expect(popup.emit).toHaveBeenCalled())
 const payload=popup.emit.mock.calls[0][1]
 expect(payload.items[1].disabled).toBe(true)
 // The popup draws a destructive item, such as Delete, in the danger color.
 expect(payload.items.map(item=>item.danger)).toEqual([false,true])
 const select=listen.mock.calls.find(([name])=>name==='action-menu-select')[1]
 select({payload:{id:'other',item:0}});expect(click).not.toHaveBeenCalled()
 select({payload:{id:payload.id,item:0}});expect(click).toHaveBeenCalledOnce()
 expect(node.style.opacity).toBe('')
 const escape=vi.fn(event=>expect(event.defaultPrevented).toBe(true));node.addEventListener('keydown',escape)
 listen.mock.calls.find(([name])=>name==='action-menu-closed')[1]({payload:payload.id})
 expect(escape).toHaveBeenCalledOnce()
 action.destroy()
})
it('keeps the HTML menu available if the popup cannot open',async()=>{
 window.__TAURI_INTERNALS__={};Window.getByLabel.mockRejectedValue(new Error('unavailable'))
 const node=document.createElement('div');node.setAttribute('role','menu');document.body.append(node)
 const action=floatingMenu(node)
 await waitFor(()=>expect(Window.getByLabel).toHaveBeenCalled())
 expect(node.style.opacity).toBe('');action.destroy()
})
it('keeps a menu that became its dialog when the popup it left closes',async()=>{
 window.__TAURI_INTERNALS__={}
 const popup={setSize:vi.fn(),setPosition:vi.fn(),emit:vi.fn(),hide:vi.fn()}
 Window.getByLabel.mockResolvedValue(popup)
 const node=document.createElement('div');node.setAttribute('role','menu');node.innerHTML='<button>Delete</button>';document.body.append(node)
 const action=floatingMenu(node)
 await waitFor(()=>expect(popup.emit).toHaveBeenCalled())
 const payload=popup.emit.mock.calls[0][1]
 const escape=vi.fn();node.addEventListener('keydown',escape)
 node.setAttribute('role','dialog')
 await waitFor(()=>expect(popup.hide).toHaveBeenCalled())
 listen.mock.calls.find(([name])=>name==='action-menu-closed')[1]({payload:payload.id})
 expect(escape).not.toHaveBeenCalled()
 expect(node.style.opacity).toBe('')
 action.destroy()
})
it('hands this window the lead of the input that chose in the popup before the item acts',async()=>{
 window.__TAURI_INTERNALS__={}
 const popup={setSize:vi.fn(),setPosition:vi.fn(),emit:vi.fn(),hide:vi.fn()}
 Window.getByLabel.mockResolvedValue(popup)
 const node=document.createElement('div');node.setAttribute('role','menu');node.innerHTML='<button>Rename</button><button data-danger>Delete</button>';document.body.append(node)
 const leads=[];node.lastChild.addEventListener('click',()=>leads.push(document.documentElement.hasAttribute('data-pointer')))
 const action=floatingMenu(node)
 await waitFor(()=>expect(popup.emit).toHaveBeenCalled())
 const {id}=popup.emit.mock.calls[0][1]
 const select=listen.mock.calls.find(([name])=>name==='action-menu-select')[1]
 // A click opened the menu, and the keys chose Delete in the popup window.
 document.documentElement.dataset.pointer=''
 select({payload:{id,item:1,via:'keys'}})
 // The keys opened the menu, and a click chose Delete.
 select({payload:{id,item:1,via:'pointer'}})
 // An older popup names no input, and the lead stays.
 select({payload:{id,item:1}})
 expect(leads).toEqual([false,true,true])
 action.destroy()
})

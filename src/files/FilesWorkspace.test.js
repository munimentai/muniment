import { cleanup, render, fireEvent, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import FilesWorkspace from './FilesWorkspace.svelte'
afterEach(() => { cleanup(); localStorage.clear() })
it('expands folders inline, keeps siblings visible, and preserves the tree while hidden', async () => {
  const invoke = vi.fn(async (command,args) => command === 'workspace_folders' ? [{label:'Home',path:'/home'}] : {
    path:args.path,entries:args.path === '/home' ? [{name:'assets',path:'/home/assets',directory:true},{name:'README.md',path:'/home/README.md',directory:false}] : [{name:'image.png',path:'/home/assets/image.png',directory:false}],
  })
  const onfile=vi.fn()
  const view=render(FilesWorkspace,{tauri:{invoke},onfile})
  const folder=await screen.findByRole('treeitem',{name:'assets'})
  await fireEvent.click(folder)
  const image=await screen.findByRole('treeitem',{name:'image.png'})
  expect(image).toHaveAttribute('aria-level','2')
  expect(screen.getByRole('treeitem',{name:'README.md'})).toHaveAttribute('aria-level','1')
  await fireEvent.click(image)
  expect(onfile).toHaveBeenCalledWith(expect.objectContaining({path:'/home/assets/image.png'}))
  await view.rerender({hidden:true})
  await view.rerender({hidden:false})
  expect(screen.getByRole('treeitem',{name:'image.png'})).toBeInTheDocument()
  await fireEvent.keyDown(image,{key:'ArrowLeft'})
  await waitFor(()=>expect(folder).toHaveFocus())
  await fireEvent.keyDown(folder,{key:'ArrowLeft'})
  expect(screen.queryByRole('treeitem',{name:'image.png'})).toBeNull()
  expect(screen.getByRole('treeitem',{name:'README.md'})).toBeInTheDocument()
  expect(invoke.mock.calls.filter(([cmd,args])=>cmd==='workspace_list' && args.path==='/home')).toHaveLength(1)
})
it('creates a file inside the selected folder and protects bulk deletion with confirmation', async () => {
  const invoke=vi.fn(async (command,args) => {
    if(command==='workspace_folders')return [{label:'Home',path:'/home'}]
    if(command==='workspace_file_action')return args.action==='new-file' ? ['/home/assets/new.js'] : []
    return {path:args.path,entries:args.path==='/home' ? [{name:'assets',path:'/home/assets',directory:true},{name:'README.md',path:'/home/README.md',directory:false}] : []}
  })
  const onfile=vi.fn()
  render(FilesWorkspace,{tauri:{invoke},onfile})
  const folder=await screen.findByRole('treeitem',{name:'assets'})
  await fireEvent.click(folder)
  await fireEvent.click(screen.getByRole('button',{name:'New file'}))
  await fireEvent.input(screen.getByRole('textbox',{name:'New file'}),{target:{value:'new.js'}})
  await fireEvent.click(screen.getByRole('button',{name:'Create'}))
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith('workspace_file_action',expect.objectContaining({action:'new-file',destination:'/home/assets',name:'new.js'})))
  await waitFor(()=>expect(onfile).toHaveBeenCalledWith({path:'/home/assets/new.js',name:'new.js',directory:false}))
  await fireEvent.click(folder,{ctrlKey:true})
  const readme=screen.getByRole('treeitem',{name:'README.md'})
  await fireEvent.click(readme,{ctrlKey:true})
  await fireEvent.contextMenu(readme)
  await fireEvent.click(screen.getByRole('menuitem',{name:'Delete 2 items…'}))
  expect(screen.getByRole('dialog',{name:'Delete 2 items?'})).toBeInTheDocument()
  expect(invoke.mock.calls.some(([c,a])=>c==='workspace_file_action' && a.action==='trash')).toBe(false)
  await fireEvent.click(screen.getByRole('button',{name:'Cancel'}))
  expect(screen.queryByRole('dialog')).toBeNull()
})
it('keeps a copied file available after changing the destination folder', async () => {
  const invoke=vi.fn(async (command,args) => {
    if(command==='workspace_folders')return [{label:'Home',path:'/home'},{label:'Project',path:'/project'}]
    if(command==='workspace_file_action')return []
    return {path:args.path,entries:[{name:'file.txt',path:args.path+'/file.txt',directory:false}]}
  })
  render(FilesWorkspace,{tauri:{invoke}})
  await fireEvent.contextMenu(await screen.findByRole('treeitem',{name:'file.txt'}))
  await fireEvent.click(screen.getByRole('menuitem',{name:'Copy'}))
  await fireEvent.change(screen.getByRole('combobox',{name:'Workspace folder'}),{target:{value:'/home'}})
  await waitFor(()=>expect(screen.getByRole('treeitem',{name:'file.txt'})).toHaveAttribute('data-path','/home/file.txt'))
  await fireEvent.contextMenu(screen.getByRole('treeitem',{name:'file.txt'}))
  await fireEvent.click(screen.getByRole('menuitem',{name:'Paste'}))
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith('workspace_file_action',expect.objectContaining({action:'paste',paths:['/project/file.txt'],destination:'/home'})))
})

it('uses explicit agent context and restores browsing state without exposing hidden folders', async () => {
  const invoke=vi.fn(async (command,args) => command==='workspace_folders' ? [{label:'Agent folder',path:'/agents/Writer-12345678'}] : {path:args.path,parent:'/agents',entries:args.path==='/agents' ? [] : [{name:'.muniment',path:args.path+'/.muniment',directory:true},{name:'agent.md',path:args.path+'/agent.md',directory:false}]})
  const props={tauri:{invoke},context:{agentId:'agent-id'}}
  const view=render(FilesWorkspace,{props})
  await screen.findByRole('treeitem',{name:'agent.md'})
  expect(invoke).toHaveBeenCalledWith('workspace_folders',{agentId:'agent-id'})
  expect(screen.queryByRole('treeitem',{name:'.muniment'})).toBeNull()
  await fireEvent.click(screen.getByRole('checkbox',{name:'Show hidden files'}))
  expect(screen.getByRole('treeitem',{name:'.muniment'})).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button',{name:'Parent folder'}))
  await waitFor(()=>expect(screen.getByRole('combobox',{name:'Workspace folder'})).toHaveValue('/agents'))
  view.unmount()
  render(FilesWorkspace,{props})
  await waitFor(()=>expect(screen.getByRole('combobox',{name:'Workspace folder'})).toHaveValue('/agents'))
})

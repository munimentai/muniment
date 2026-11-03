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
it('shows the reading state while its folders wait on a folder prompt', async () => {
  let answer
  const invoke=vi.fn((command,args) => command==='workspace_folders' ? new Promise(resolve => { answer=resolve }) : Promise.resolve({path:args.path,entries:[{name:'notes.md',path:args.path+'/notes.md',directory:false}]}))
  render(FilesWorkspace,{tauri:{invoke}})
  expect(screen.getByRole('status')).toHaveTextContent('Reading folder…')
  expect(screen.queryByText(/No files here yet/)).toBeNull()
  answer([{label:'Home',path:'/home'}])
  expect(await screen.findByRole('treeitem',{name:'notes.md'})).toBeInTheDocument()
  expect(screen.queryByText('Reading folder…')).toBeNull()
})
it('leaves the reading state when its folders cannot be read', async () => {
  const invoke=vi.fn(async () => { throw 'Choose a Home folder first.' })
  render(FilesWorkspace,{tauri:{invoke}})
  expect(await screen.findByRole('alert')).toHaveTextContent('Choose a Home folder first.')
  expect(screen.queryByText('Reading folder…')).toBeNull()
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
  // Delete is the menu's one destructive item, marked so the native popup draws it in the danger color too.
  expect(screen.getAllByRole('menuitem').filter(item=>item.hasAttribute('data-danger')).map(item=>item.textContent)).toEqual(['Delete 2 items…'])
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
it('follows its thread folder when the thread takes its title, and again on a rename', async () => {
  let folder = '/home/sessions/New_thread-c3fa5c5a'
  const lists = {
    '/home/sessions/New_thread-c3fa5c5a': [{name:'drafts',path:'/home/sessions/New_thread-c3fa5c5a/drafts',directory:true}],
    '/home/sessions/Plan_the_week-c3fa5c5a': [{name:'drafts',path:'/home/sessions/Plan_the_week-c3fa5c5a/drafts',directory:true},{name:'plan.md',path:'/home/sessions/Plan_the_week-c3fa5c5a/plan.md',directory:false}],
    '/home/sessions/Week_plan-c3fa5c5a': [{name:'week.md',path:'/home/sessions/Week_plan-c3fa5c5a/week.md',directory:false}],
  }
  const invoke = vi.fn(async (command,args) => {
    if (command === 'workspace_folders') return [{label:'Muniment folder',path:'/home'},{label:'Thread workspace',path:folder}]
    if (!lists[args.path]) throw 'This folder is unavailable.'
    return {path:args.path,parent:'/home/sessions',entries:lists[args.path]}
  })
  const onfolder = vi.fn()
  const view = render(FilesWorkspace,{props:{tauri:{invoke},context:{threadId:'t1',threadTitle:'New thread'},onfolder}})
  expect(await screen.findByRole('treeitem',{name:'drafts'})).toBeInTheDocument()
  // A run works in the folder with no title, and core names the folder when it ends.
  await view.rerender({props:{context:{threadId:'t1'}}})
  folder = '/home/sessions/Plan_the_week-c3fa5c5a'
  await view.rerender({props:{context:{threadId:'t1',threadTitle:'Plan the week'}}})
  expect(await screen.findByRole('treeitem',{name:'plan.md'})).toBeInTheDocument()
  expect(invoke).toHaveBeenCalledWith('workspace_folders',{threadId:'t1',threadTitle:'Plan the week'})
  expect(onfolder).toHaveBeenLastCalledWith('/home/sessions/Plan_the_week-c3fa5c5a')
  await fireEvent.click(screen.getByRole('button',{name:'Refresh files'}))
  expect(screen.queryByText('This folder is unavailable.')).toBeNull()
  folder = '/home/sessions/Week_plan-c3fa5c5a'
  await view.rerender({props:{context:{threadId:'t1',threadTitle:'Week plan'}}})
  expect(await screen.findByRole('treeitem',{name:'week.md'})).toBeInTheDocument()
  expect(onfolder).toHaveBeenLastCalledWith('/home/sessions/Week_plan-c3fa5c5a')
})

it('keeps a folder the person chose when the thread takes its title', async () => {
  const invoke = vi.fn(async (command,args) => command === 'workspace_folders'
    ? [{label:'Muniment folder',path:'/home'},{label:'Thread workspace',path:args.threadTitle ? '/home/sessions/Plan-c3fa5c5a' : '/home/sessions/Untitled-c3fa5c5a'}]
    : {path:args.path,entries:[{name:`${args.path.split('/').at(-1)}.md`,path:`${args.path}/x.md`,directory:false}]})
  const view = render(FilesWorkspace,{props:{tauri:{invoke},context:{threadId:'t1'}}})
  expect(await screen.findByRole('treeitem',{name:'Untitled-c3fa5c5a.md'})).toBeInTheDocument()
  await fireEvent.change(screen.getByRole('combobox',{name:'Workspace folder'}),{target:{value:'/home'}})
  expect(await screen.findByRole('treeitem',{name:'home.md'})).toBeInTheDocument()
  await view.rerender({props:{context:{threadId:'t1',threadTitle:'Plan'}}})
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('workspace_folders',{threadId:'t1',threadTitle:'Plan'}))
  expect(screen.getByRole('treeitem',{name:'home.md'})).toBeInTheDocument()
  expect(invoke.mock.calls.filter(([command,args]) => command === 'workspace_list' && args.path === '/home/sessions/Plan-c3fa5c5a')).toHaveLength(0)
})

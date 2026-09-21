import { render, screen, fireEvent, waitFor, cleanup, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import ExtendSection from './ExtendSection.svelte'
beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () { this.open = true }
  HTMLDialogElement.prototype.close = function () { this.open = false; this.dispatchEvent(new Event('close')) }
})
afterEach(cleanup)
it('searches and filters the MCP catalog before displaying a page', async () => {
  render(ExtendSection, { tauri:{invoke:vi.fn(async()=>({items:[],threads:{}}))} })
  await fireEvent.input(screen.getByRole('searchbox', {name:'Search MCP servers'}), {target:{value:'Airtable'}})
  expect(screen.getByText('Airtable', {selector:'strong'})).toBeInTheDocument()
  expect(screen.queryByRole('navigation', {name:'Catalog pages'})).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button',{name:'Filters'}))
  await fireEvent.change(screen.getByLabelText('Category'), {target:{value:'legal'}})
  expect(screen.getByText('No servers match these filters.')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', {name:'Clear filters'}))
  expect(screen.getByLabelText('Category')).toHaveValue('')
  expect(screen.queryByLabelText('Connection')).not.toBeInTheDocument()
  expect(document.body.textContent).not.toMatch(/Anthropic|Includes all/)
})
it('previews a source before installing selected skills', async () => {
  const invoke=vi.fn(async(_, {action})=> action==='preview' ? {id:'preview',name:'Review',description:'Review text',version:'abc123',skills:[{name:'review',path:'SKILL.md',description:'Review text'}],extensions:[],servers:{}} : {items:[],threads:{}})
  render(ExtendSection,{tauri:{invoke}})
  await fireEvent.click(screen.getByRole('tab',{name:'Skills'}))
  await waitFor(()=>expect(screen.getByRole('button',{name:'Add source'})).not.toBeDisabled())
  await fireEvent.click(screen.getByRole('button',{name:'Add source'}))
  await fireEvent.input(screen.getByLabelText('GitHub repository, local folder, or archive'),{target:{value:'https://github.com/example/skills'}})
  await fireEvent.click(screen.getByRole('button',{name:'Review package'}))
  await screen.findByRole('button',{name:'Install selected'})
  expect(invoke.mock.calls.some(([,arg])=>arg.action==='install')).toBe(false)
  await fireEvent.click(screen.getByRole('button',{name:'Install selected'}))
  expect(invoke).toHaveBeenCalledWith('extend_command',{action:'install',data:{previewId:'preview',skills:['SKILL.md'],replaceId:null}})
})

it('shows twelve popular cards before the rest and supports name sorting', async () => {
  render(ExtendSection, {tauri:{invoke:vi.fn(async()=>({items:[]}))}})
  expect(screen.getByLabelText('Popular MCP servers').querySelectorAll('article')).toHaveLength(12)
  await fireEvent.click(screen.getByRole('button',{name:'Filters'}))
  expect(screen.getByLabelText('Sort')).toHaveValue('popular')
  await fireEvent.change(screen.getByLabelText('Sort'), {target:{value:'name'}})
  expect(screen.queryByLabelText('Popular MCP servers')).not.toBeInTheDocument()
})

it('opens provider details inside the app and adds the server from the dialog', async () => {
  const show = vi.fn(function () { this.open = true })
  const oldShow = HTMLDialogElement.prototype.showModal
  HTMLDialogElement.prototype.showModal = show
  const oldClose = HTMLDialogElement.prototype.close
  HTMLDialogElement.prototype.close = function () { this.open = false; this.dispatchEvent(new Event('close')) }
  try {
    const invoke = vi.fn(async (_, {action, data}) => action === 'server' ? {items:[{id:'saved',...data}]} : action === 'auth' ? {status:'connected',tools:[]} : {items:[]})
    render(ExtendSection, {tauri:{invoke}})
    await fireEvent.input(screen.getByRole('searchbox'), {target:{value:'Notion'}})
    await fireEvent.click(screen.getAllByRole('button', {name:'Details'})[0])
    const dialog = await screen.findByRole('dialog', {name:'Notion'})
    expect(within(dialog).getByText('Server URL')).toBeInTheDocument()
    expect(within(dialog).getByText('Publisher')).toBeInTheDocument()
    expect(within(dialog).getByRole('link')).toHaveAttribute('target','_blank')
    expect(invoke.mock.calls.every(([,args]) => args.action === 'read')).toBe(true)
    await waitFor(() => expect(within(dialog).getByRole('button', {name:'Connect'})).not.toBeDisabled())
    await fireEvent.click(within(dialog).getByRole('button', {name:'Connect'}))
    expect(screen.queryByRole('dialog', {name:'Notion'})).not.toBeInTheDocument()
    expect(screen.queryByLabelText('Name')).not.toBeInTheDocument()
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('extend_command',{action:'auth',data:{id:'saved'}}))
  } finally { HTMLDialogElement.prototype.showModal = oldShow; HTMLDialogElement.prototype.close = oldClose }
})

it('starts OAuth after saving a catalog connection and opens managed storage', async () => {
  const invoke = vi.fn(async (_, {action,data}) => action === 'auth' ? {status:'connected',tools:[]} : action === 'server' ? {items:[{id:'saved',...data}]} : {items:[]})
  render(ExtendSection, {tauri:{invoke}})
  await fireEvent.input(screen.getByRole('searchbox'), {target:{value:'Notion'}})
  await waitFor(() => expect(screen.getAllByRole('button',{name:'Connect'})[0]).not.toBeDisabled())
  await fireEvent.click(screen.getAllByRole('button',{name:'Connect'})[0])
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  expect(screen.queryByLabelText('Authentication')).not.toBeInTheDocument()
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('extend_command',{action:'auth',data:{id:'saved'}}))
  await fireEvent.click(screen.getByRole('tab',{name:'Skills'}))
  await waitFor(() => expect(screen.getByRole('button',{name:'Open folder'})).not.toBeDisabled())
  await fireEvent.click(screen.getByRole('button',{name:'Open folder'}))
  expect(invoke).toHaveBeenCalledWith('extend_command',{action:'open_folder',data:{}})
})

it('hides filters initially and opens custom setup as a dialog', async () => {
  render(ExtendSection,{tauri:{invoke:vi.fn(async()=>({items:[]}))}})
  expect(screen.queryByLabelText('Category')).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button',{name:'Filters'}))
  expect(screen.getByLabelText('Category')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button',{name:'Filters'}))
  expect(screen.queryByLabelText('Category')).not.toBeInTheDocument()
  await waitFor(()=>expect(screen.getByRole('button',{name:'Custom'})).not.toBeDisabled())
  await fireEvent.click(screen.getByRole('button',{name:'Custom'}))
  expect(await screen.findByRole('dialog',{name:'Add MCP server'})).toHaveAttribute('open')
  await fireEvent.click(screen.getByRole('button',{name:'Cancel'}))
  expect(screen.queryByRole('dialog',{name:'Add MCP server'})).not.toBeInTheDocument()
})

it('keeps installed MCPs in the catalog with a switch and detail controls', async () => {
  const { catalog } = await import('./catalog.js')
  const source = catalog.find(entry => entry.name === 'Notion')
  const item = {id:'notion',kind:'mcp',name:'Notion',source:source.source,enabled:true,definition:{url:source.url}}
  const invoke = vi.fn(async (_, {action,data}) => ({items:[action === 'toggle' ? {...item,enabled:data.enabled} : item]}))
  render(ExtendSection,{tauri:{invoke}})
  const toggle = await screen.findByRole('switch',{name:'Use Notion in chats'})
  expect(toggle).toHaveAttribute('aria-checked','true')
  expect(screen.queryByRole('heading',{name:'Installed'})).not.toBeInTheDocument()
  expect(screen.getAllByText('Notion',{selector:'strong'})).toHaveLength(1)
  expect(toggle.closest('article')).toHaveClass('connected')
  await fireEvent.click(toggle)
  await waitFor(()=>expect(toggle).toHaveAttribute('aria-checked','false'))
  expect(invoke).toHaveBeenCalledWith('extend_command',{action:'toggle',data:{id:'notion',enabled:false}})
  await fireEvent.click(screen.getByRole('button',{name:'Filters'}))
  await fireEvent.change(screen.getByLabelText('Show'),{target:{value:'installed'}})
  expect(screen.queryByText('Canva',{selector:'strong'})).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button',{name:'Details'}))
  const dialog = await screen.findByRole('dialog',{name:'Notion'})
  expect(within(dialog).getByRole('button',{name:'Configure'})).toBeInTheDocument()
  expect(within(dialog).getByRole('button',{name:'Uninstall'})).toBeInTheDocument()
})

it('keeps custom and excluded installed MCPs available in the catalog', async () => {
  render(ExtendSection,{tauri:{invoke:vi.fn(async()=>({items:[{id:'custom',kind:'mcp',name:'Custom docs',source:'',enabled:true,definition:{url:'https://example.com/mcp'}}]}))}})
  await screen.findByRole('tab',{name:'MCP servers (493)'})
  await fireEvent.input(screen.getByRole('searchbox'),{target:{value:'Custom docs'}})
  expect(await screen.findByRole('switch',{name:'Use Custom docs in chats'})).toBeInTheDocument()
})

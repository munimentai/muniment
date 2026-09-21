import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import ExtendSection from './ExtendSection.svelte'
afterEach(cleanup)
it('searches and filters the MCP catalog before displaying a page', async () => {
  render(ExtendSection, { tauri:{invoke:vi.fn(async()=>({items:[],threads:{}}))} })
  await fireEvent.input(screen.getByRole('searchbox', {name:'Search MCP servers'}), {target:{value:'Airtable'}})
  expect(screen.getByText('Airtable', {selector:'strong'})).toBeInTheDocument()
  expect(screen.queryByRole('navigation', {name:'Catalog pages'})).not.toBeInTheDocument()
  await fireEvent.change(screen.getByLabelText('Connection'), {target:{value:'local'}})
  expect(screen.getByText('No servers match these filters.')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', {name:'Clear filters'}))
  expect(screen.getByLabelText('Connection')).toHaveValue('')
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
  expect(screen.getByLabelText('Sort')).toHaveValue('popular')
  await fireEvent.change(screen.getByLabelText('Sort'), {target:{value:'name'}})
  expect(screen.queryByLabelText('Popular MCP servers')).not.toBeInTheDocument()
})

import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
const state = vi.hoisted(() => ({value:'', change:()=>{}, command:null}))
vi.mock('./code-editor.js', () => ({monaco:{Uri:{from:x=>x},KeyMod:{CtrlCmd:1},KeyCode:{KeyS:2},editor:{setTheme:()=>{},createModel:value=>{state.value=value;return {getValue:()=>state.value,setValue:value=>{state.value=value;state.change()},onDidChangeContent:fn=>{state.change=fn;return {dispose(){}}},dispose(){}}},create:()=>({addCommand:(_,fn)=>state.command=fn,dispose(){}})}}}))
import FileEditor from './FileEditor.svelte'
afterEach(cleanup)
it('saves edits with the original revision and retains the draft after a conflict', async () => {
  window.matchMedia = () => ({matches:true,addEventListener(){},removeEventListener(){}})
  const invoke=vi.fn(async command => {
    if(command==='workspace_read_text') return {path:'/work/app.js',content:'const x = 1',revision:'original'}
    throw new Error('The file changed on disk.')
  })
  const ondirty=vi.fn()
  render(FileEditor,{file:{name:'app.js',path:'/work/app.js'},tauri:{invoke},ondirty})
  await waitFor(()=>expect(screen.getByRole('button',{name:'Reload'})).toBeEnabled())
  state.value='const x = 2';state.change()
  await fireEvent.click(screen.getByRole('button',{name:'Save'}))
  await waitFor(()=>expect(screen.getByRole('alert')).toHaveTextContent('The file changed on disk.'))
  expect(invoke).toHaveBeenCalledWith('workspace_save_text',{path:'/work/app.js',content:'const x = 2',revision:'original'})
  expect(state.value).toBe('const x = 2')
  expect(ondirty).toHaveBeenLastCalledWith(true)
  await fireEvent.click(screen.getByRole('button',{name:'Reload'}))
  expect(screen.getByRole('dialog')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button',{name:'Cancel'}))
  expect(state.value).toBe('const x = 2')
  invoke.mockResolvedValueOnce({revision:'saved'})
  await state.command()
  await waitFor(()=>expect(screen.getByRole('button',{name:'Save'})).toBeDisabled())
  expect(ondirty).toHaveBeenLastCalledWith(false)
})

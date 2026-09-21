import {afterEach,expect,it,vi} from 'vitest'
import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import CatalogActions from './CatalogActions.svelte'
afterEach(cleanup)
it('renames an item and closes the menu after saving',async()=>{
 const onaction=vi.fn().mockResolvedValue()
 render(CatalogActions,{name:'Report',onaction})
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Report'}))
 await fireEvent.click(screen.getByRole('menuitem',{name:'Rename'}))
 await fireEvent.input(screen.getByRole('textbox',{name:'Name'}),{target:{value:'New report'}})
 await fireEvent.submit(screen.getByRole('textbox',{name:'Name'}).closest('form'))
 await waitFor(()=>expect(onaction).toHaveBeenCalledWith('rename','New report'))
 expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
})
it('confirms deletion and keeps errors visible',async()=>{
 const onaction=vi.fn().mockRejectedValue(new Error('Run is active.'))
 render(CatalogActions,{name:'Scout',onaction})
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 await fireEvent.click(screen.getByRole('menuitem',{name:'Delete'}))
 expect(onaction).not.toHaveBeenCalled()
 await fireEvent.click(screen.getByRole('button',{name:'Confirm delete'}))
 expect(await screen.findByRole('alert')).toHaveTextContent('Run is active.')
})
it('restores archived items and dismisses with Escape',async()=>{
 const onaction=vi.fn().mockResolvedValue()
 render(CatalogActions,{name:'Scout',archived:true,onaction})
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 await fireEvent.click(screen.getByRole('menuitem',{name:'Restore'}))
 expect(onaction).toHaveBeenCalledWith('restore','')
 await waitFor(()=>expect(screen.queryByRole('menu')).not.toBeInTheDocument())
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 const outerEscape=vi.fn()
 document.addEventListener('keydown',outerEscape)
 await fireEvent.keyDown(document,{key:'Escape'})
 expect(outerEscape).not.toHaveBeenCalled()
 document.removeEventListener('keydown',outerEscape)
 expect(screen.queryByRole('menu')).not.toBeInTheDocument()
})

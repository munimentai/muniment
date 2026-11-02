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
 expect(screen.getByRole('button',{name:'Confirm delete'})).toHaveClass('danger')
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
it('hands Delete to its owner when the owner confirms it',async()=>{
 const onaction=vi.fn().mockResolvedValue()
 render(CatalogActions,{name:'Scout',confirmDelete:false,onaction})
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 await fireEvent.click(screen.getByRole('menuitem',{name:'Delete'}))
 expect(onaction).toHaveBeenCalledWith('delete','')
 expect(screen.queryByRole('button',{name:'Confirm delete'})).not.toBeInTheDocument()
})
it('draws Delete, the one destructive item, in the danger color on agent, artifact and creation cards',async()=>{
 const source=(await import('./CatalogActions.svelte?raw')).default
 render(CatalogActions,{name:'Scout',confirmDelete:false,extraActions:[{id:'profile',label:'Open profile',icon:'user'}],onaction:vi.fn()})
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 const remove=screen.getByRole('menuitem',{name:'Delete'})
 expect(remove).toHaveAttribute('data-danger')
 expect(screen.getAllByRole('menuitem').filter(item=>item.hasAttribute('data-danger'))).toEqual([remove])
 expect(source).toMatch(/\.catalog-menu > button\[data-danger\]\s*\{\s*color:\s*var\(--danger\)\s*\}/)
})
it('shows the first item on the first Down arrow in a menu a click opened, and the first item again after the keys opened it',async()=>{
 render(CatalogActions,{name:'Scout',onaction:vi.fn()})
 // A click opens the menu: the pointer leads, and no item shows yet.
 document.documentElement.dataset.pointer=''
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 expect(screen.getByRole('menuitem',{name:'Rename'})).toHaveFocus()
 await fireEvent.keyDown(document.activeElement,{key:'ArrowDown'})
 expect(screen.getByRole('menuitem',{name:'Rename'})).toHaveFocus()
 await fireEvent.keyDown(document.activeElement,{key:'ArrowDown'})
 expect(screen.getByRole('menuitem',{name:'Archive'})).toHaveFocus()
 await fireEvent.keyDown(document,{key:'Escape'})
 // Up from a click reaches the last item.
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 await fireEvent.keyDown(document.activeElement,{key:'ArrowUp'})
 expect(screen.getByRole('menuitem',{name:'Delete'})).toHaveFocus()
 await fireEvent.keyDown(document,{key:'Escape'})
 // The keys opened it: the first item shows, and Down moves to the second.
 delete document.documentElement.dataset.pointer
 await fireEvent.click(screen.getByRole('button',{name:'Actions for Scout'}))
 await fireEvent.keyDown(document.activeElement,{key:'ArrowDown'})
 expect(screen.getByRole('menuitem',{name:'Archive'})).toHaveFocus()
})

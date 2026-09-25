import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelRouterSection from './ModelRouterSection.svelte'
afterEach(cleanup)
it('selects only connected classifiers and keeps account balancing on', async () => {
 const settings={enabled:true,options:[],routes:[],accounts:[],classifier_connections:[{id:'jev',name:'Jev',catalog_id:'jev',active:true},{id:'semif',name:'SemIf',catalog_id:'semif',active:false}]}
 const invoke=vi.fn(async()=>settings)
 render(ModelRouterSection,{tauri:{invoke},settings,onsettings:vi.fn(),inventory:{providers:[],default_provider:'muniment-router',default_model:'auto'}})
 await fireEvent.click(screen.getByRole('button',{name:'Classifier model: Jev'}))
 expect(screen.getAllByRole('menuitemradio')).toHaveLength(2)
 expect(screen.getByRole('menuitemradio',{name:'Jev'})).toHaveAttribute('aria-checked','true')
 expect(invoke).not.toHaveBeenCalled()
 await fireEvent.click(screen.getByRole('menuitemradio',{name:'SemIf'}))
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith('model_router_select_classifier',{id:'semif'}))
 expect(invoke).not.toHaveBeenCalledWith('model_router_set_enabled',{enabled:false})
 expect(screen.getByText(/Account balancing stays on/)).toBeInTheDocument()
})

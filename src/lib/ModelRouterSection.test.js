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
it('lists the Ollama server decision models and connects one when it is chosen', async () => {
 const base='https://ollama.example/v1/systemone'
 const jev={id:'jev',name:'Jev',catalog_id:'jev',active:true,connection:{kind:'typesafe',model:'jev-latest'}}
 const settings={enabled:true,options:[{key:'fast'}],routes:[],accounts:[],classifier_connections:[jev]}
 const connected={...settings,classifier_connections:[jev,{id:'c2',name:'Clef Flash · Ollama',catalog_id:'clef',active:false,connection:{kind:'endpoint',model:'clef-flash:latest',base_url:base}}]}
 const invoke=vi.fn(async(command)=>command==='model_router_connect_classifier'?connected:command==='local_mode_provider_inventory'?{providers:[]}:settings)
 const inventory={providers:[{id:'ollama',name:'Ollama',source:'local',base_url:'https://ollama.example/v1',models:[{id:'gpt-oss:latest'},{id:'clef-flash:latest'},{id:'nimble:latest'}]}],default_provider:'muniment-router',default_model:'auto'}
 render(ModelRouterSection,{tauri:{invoke},settings,onsettings:vi.fn(),inventory})
 await fireEvent.click(screen.getByRole('button',{name:'Classifier model: Jev'}))
 // Chat models stay out; the decision models join the saved connection.
 expect(screen.getAllByRole('menuitemradio').map(item=>item.textContent.trim())).toEqual(['Jev','Clef Flash · Ollama','Nimble · Ollama'])
 await fireEvent.click(screen.getByRole('menuitemradio',{name:'Clef Flash · Ollama'}))
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith('model_router_select_classifier',{id:'c2'}))
 expect(invoke).toHaveBeenCalledWith('model_router_connect_classifier',{catalogId:'clef',name:'Clef Flash · Ollama',model:'clef-flash:latest',baseUrl:base,apiKey:null,keyProvider:'ollama'})
})

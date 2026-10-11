import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelRouterSection from './ModelRouterSection.svelte'
afterEach(cleanup)
it('selects only connected classifiers and keeps account balancing on', async () => {
 const settings={enabled:true,options:[],routes:[],accounts:[],classifier_connections:[{id:'jev',name:'Jev',catalog_id:'jev',active:true},{id:'semif',name:'SemIf',catalog_id:'semif',active:false}]}
 const invoke=vi.fn(async()=>settings)
 render(ModelRouterSection,{tauri:{invoke},settings,onsettings:vi.fn(),inventory:{providers:[],default_provider:'muniment-router',default_model:'auto'}})
 await fireEvent.click(screen.getByRole('button',{name:'Decision model: Jev'}))
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
 await fireEvent.click(screen.getByRole('button',{name:'Decision model: Jev'}))
 // Chat models stay out; the decision models join the saved connection.
 expect(screen.getAllByRole('menuitemradio').map(item=>item.textContent.trim())).toEqual(['Jev','Clef Flash · Ollama','Nimble · Ollama'])
 await fireEvent.click(screen.getByRole('menuitemradio',{name:'Clef Flash · Ollama'}))
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith('model_router_select_classifier',{id:'c2'}))
 expect(invoke).toHaveBeenCalledWith('model_router_connect_classifier',{catalogId:'clef',name:'Clef Flash · Ollama',model:'clef-flash:latest',baseUrl:base,apiKey:null,keyProvider:'ollama'})
})
it('offers the decision models a llama.cpp server reports and sends that server its own key',async()=>{
 const base='http://localhost:8080/v1/systemone'
 const jev={id:'jev',name:'Jev',catalog_id:'jev',active:true,connection:{kind:'typesafe',model:'jev-latest'}}
 const settings={enabled:true,options:[{key:'fast'}],routes:[],accounts:[],classifier_connections:[jev]}
 const invoke=vi.fn(async(command)=>command==='model_router_connect_classifier'?settings:command==='local_mode_provider_inventory'?{providers:[]}:settings)
 const inventory={providers:[{id:'llama',name:'llama.cpp',source:'custom',base_url:'http://localhost:8080/v1',models:[{id:'qwen3'}],decision_models:['kev-4b']}],default_provider:'muniment-router',default_model:'auto'}
 render(ModelRouterSection,{tauri:{invoke},settings,onsettings:vi.fn(),inventory})
 await fireEvent.click(screen.getByRole('button',{name:'Decision model: Jev'}))
 expect(screen.getAllByRole('menuitemradio').map(item=>item.textContent.trim())).toEqual(['Jev','kev-4b · llama.cpp'])
 await fireEvent.click(screen.getByRole('menuitemradio',{name:'kev-4b · llama.cpp'}))
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith('model_router_connect_classifier',{catalogId:'llama',name:'kev-4b · llama.cpp',model:'kev-4b',baseUrl:base,apiKey:null,keyProvider:'llama'}))
})
it('turns assistance on with its own decision model apart from routing',async()=>{
 const jev={id:'jev',name:'Jev · TypeSafe API',catalog_id:'jev',active:true}
 const clef={id:'clef',name:'Clef Flash · Ollama',catalog_id:'clef',active:false}
 const settings={enabled:true,options:[{key:'fast'}],routes:[],accounts:[],classifier_connections:[jev,clef],assist:{enabled:false,connection:''}}
 const on={...settings,assist:{enabled:true,connection:''}}
 const invoke=vi.fn(async(command,args)=>command==='model_router_set_assist'?{...settings,assist:{enabled:args.enabled,connection:args.id}}:settings)
 const onsettings=vi.fn()
 const {rerender}=render(ModelRouterSection,{tauri:{invoke},settings,onsettings,inventory:{providers:[],default_provider:'muniment-router',default_model:'auto'}})
 await fireEvent.click(screen.getByRole('switch',{name:'Use assistance'}))
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith('model_router_set_assist',{enabled:true,id:''}))
 await rerender({settings:on})
 await fireEvent.click(screen.getByRole('button',{name:'Assistance decision model: Same as model routing (Jev · TypeSafe API)'}))
 await fireEvent.click(screen.getByRole('menuitemradio',{name:'Clef Flash · Ollama'}))
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith('model_router_set_assist',{enabled:true,id:'clef'}))
 expect(invoke).not.toHaveBeenCalledWith('model_router_select_classifier',expect.anything())
})

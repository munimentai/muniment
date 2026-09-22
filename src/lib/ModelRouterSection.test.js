import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ModelRouterSection from './ModelRouterSection.svelte'
afterEach(cleanup)
it('offers tested self-hosted classifiers without changing Jev until saved', async () => {
 const settings={enabled:true,options:[],routes:[],accounts:[],min_confidence:.6,classifier:{kind:'typesafe',model:'jev-latest',configured:true}}
 const invoke=vi.fn(async()=>settings)
 render(ModelRouterSection,{tauri:{invoke},settings,onsettings:vi.fn(),inventory:{providers:[]}})
 await fireEvent.click(screen.getByText('Classifier and fallback'))
 expect(screen.getByRole('button',{name:/TypeSafe Jev/})).toHaveAttribute('aria-pressed','true')
 await fireEvent.click(screen.getByRole('button',{name:/SemIf/}))
 expect(screen.getByLabelText('Classifier URL').closest('li')).toContainElement(screen.getByRole('button',{name:/SemIf/}))
 expect(invoke).not.toHaveBeenCalled()
 expect(screen.getByLabelText('Model ID')).toHaveValue('semif-qwen3.5-4b')
 await fireEvent.input(screen.getByLabelText('Classifier URL'),{target:{value:'http://127.0.0.1:52020/v1/systemone'}})
 await fireEvent.click(screen.getByRole('button',{name:/Decider 2B/}))
 expect(screen.getByLabelText('Classifier URL')).toHaveValue('http://127.0.0.1:52020/v1/systemone')
 expect(screen.getByLabelText('Model ID')).toHaveValue('decider-2b')
 expect(screen.getByLabelText('Classifier URL').closest('li')).toContainElement(screen.getByRole('button',{name:/Decider 2B/}))
 await fireEvent.click(screen.getByRole('button',{name:'Save classifier'}))
 expect(invoke).toHaveBeenCalledWith('model_router_set_classifier',{kind:'endpoint',model:'decider-2b',apiKey:'',baseUrl:'http://127.0.0.1:52020/v1/systemone',family:null})
})

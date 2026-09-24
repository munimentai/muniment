import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import Capacity from './Capacity.svelte'
afterEach(cleanup)
it('shows separate allowance meters and account exclusions', async () => {
  const invoke = vi.fn(async () => ({enabled:true,running:true,accounts:[
    {id:'api',source:'key',label:'API key',family:'openai',windows:[]},
    {id:'a',source:'account',label:'Work',family:'openai',windows:[{label:'Weekly',remaining_percent:20}]},
    {id:'b',source:'account',label:'Personal',family:'anthropic',exclusion_reason:'Account is turned off.',windows:[]},
  ]}))
  const onclose = vi.fn()
  render(Capacity,{tauri:{invoke},onclose,onmanage:vi.fn()})
  await screen.findByText('OpenAI · Work')
  expect(screen.queryByText('API key')).not.toBeInTheDocument()
  expect(screen.getByRole('meter')).toHaveAttribute('aria-valuenow','20')
  expect(screen.getByRole('meter')).toHaveAttribute('data-level','low')
  expect(screen.getByText('Account is turned off.')).toBeInTheDocument()
  expect(screen.getByText('Allowance unavailable')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button',{name:'Close capacity'}))
  expect(onclose).toHaveBeenCalledOnce()
})
it('shows when balancing is disabled', async () => {
  render(Capacity,{tauri:{invoke:vi.fn(async()=>({enabled:false,accounts:[]}))},onclose:vi.fn(),onmanage:vi.fn()})
  expect(await screen.findByText('Account balancing is off.')).toBeInTheDocument()
})
it('skips the poll while the window is hidden and refreshes once when it shows', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true })
  let hidden = false
  const visibility = vi.spyOn(document, 'hidden', 'get').mockImplementation(() => hidden)
  try {
    const invoke = vi.fn(async () => ({enabled:false,accounts:[]}))
    render(Capacity,{tauri:{invoke},onclose:vi.fn(),onmanage:vi.fn()})
    await screen.findByText('Account balancing is off.')
    expect(invoke).toHaveBeenCalledTimes(1)
    hidden = true
    await vi.advanceTimersByTimeAsync(45000)
    expect(invoke).toHaveBeenCalledTimes(1)
    hidden = false
    document.dispatchEvent(new Event('visibilitychange'))
    expect(invoke).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(15000)
    expect(invoke).toHaveBeenCalledTimes(3)
  } finally {
    visibility.mockRestore()
    vi.useRealTimers()
  }
})

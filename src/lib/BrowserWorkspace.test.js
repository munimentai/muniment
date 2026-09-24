import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import BrowserWorkspace from './BrowserWorkspace.svelte'
let invoke
beforeEach(() => {
  localStorage.clear()
  window.__TAURI__ = { event: { listen: vi.fn(async () => () => {}) } }
  globalThis.ResizeObserver = class { observe() {} disconnect() {} }
  invoke = vi.fn(async (command, args) => {
    if (command === 'artifact_list') return []
    if (command === 'artifact_save') return { ...args, id: '01900000-0000-7000-8000-000000000001' }
    if (command === 'browser_command') return { origin: 'https://example.org', url: 'https://example.org/saved', allowed: false }
  })
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 220, y: 170, width: 800, height: 480 })
})
afterEach(() => { cleanup(); vi.restoreAllMocks() })
describe('shared browser', () => {
  it('shows the retained native page address when reopened', async () => {
    render(BrowserWorkspace, { tauri: { invoke }, onclose: vi.fn() })
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Website address' })).toHaveValue('https://example.org/saved'))
  })
  it('hides the native page behind settings and on close', async () => {
    const view = render(BrowserWorkspace, { tauri: { invoke }, onclose: vi.fn() })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_view', expect.objectContaining({ label: 'browser' })))
    await view.rerender({ suspended: true })
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith('browser_view', { label: null, bounds: null, artifactId: null }))
    await view.rerender({ suspended: false })
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith('browser_view', expect.objectContaining({ label: 'browser' })))
    view.unmount()
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith('browser_view', { label: null, bounds: null, artifactId: null }))
  })
  it('moves the native page once per frame for a burst of resizes', async () => {
    let resize
    globalThis.ResizeObserver = class { constructor(callback) { resize = callback } observe() {} disconnect() {} }
    render(BrowserWorkspace, { tauri: { invoke }, onclose: vi.fn() })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_view', expect.objectContaining({ label: 'browser' })))
    await new Promise(resolve => requestAnimationFrame(resolve))
    const views = () => invoke.mock.calls.filter(([name]) => name === 'browser_view').length
    const before = views()
    resize()
    window.dispatchEvent(new Event('resize'))
    resize()
    expect(views()).toBe(before)
    await new Promise(resolve => requestAnimationFrame(resolve))
    await new Promise(resolve => setTimeout(resolve, 50))
    expect(views()).toBe(before + 1)
  })
  it('loads a requested chat link after positioning the native browser', async () => {
    const request = {url:'https://xtermjs.org/'}
    const onnavigationhandled = vi.fn()
    render(BrowserWorkspace,{tauri:{invoke},navigation:request,onnavigationhandled})
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_command',{request:{view:'browser',action:'navigate',value:request.url}}))
    await waitFor(() => expect(onnavigationhandled).toHaveBeenCalledWith(request))
    const viewIndex = invoke.mock.calls.findIndex(([name]) => name === 'browser_view')
    const navigationIndex = invoke.mock.calls.findIndex(([name,args]) => name === 'browser_command' && args.request.action === 'navigate')
    expect(viewIndex).toBeLessThan(navigationIndex)
  })
  it('navigates on Enter without manual control or page-reading buttons', async () => {
    render(BrowserWorkspace, {tauri:{invoke}})
    const input = screen.getByRole('textbox',{name:'Website address'})
    await fireEvent.input(input,{target:{value:'example.com'}})
    await fireEvent.submit(input.closest('form'))
    expect(invoke).toHaveBeenCalledWith('browser_command',{request:{view:'browser',action:'navigate',value:'https://example.com'}})
    for (const name of ['Go','Allow agent control','Read page','Close']) expect(screen.queryByRole('button',{name})).toBeNull()
  })
  it('shows an empty chat artifact list without an editor', async () => {
    render(BrowserWorkspace,{tauri:{invoke},artifacts:true})
    expect(await screen.findByText('Artifacts created in chat appear here.')).toBeInTheDocument()
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(invoke.mock.calls.some(([command]) => command === 'artifact_save')).toBe(false)
  })
  it('opens saved session artifacts and refreshes the preview when chat updates one', async () => {
    const id = '01900000-0000-7000-8000-000000000001'
    invoke.mockImplementation(async command => command === 'artifact_list' ? [{id,name:'Counter'}] : {})
    render(BrowserWorkspace,{tauri:{invoke},artifacts:true})
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_view',expect.objectContaining({label:'artifact',artifactId:id})))
    invoke.mockClear()
    const listener = window.__TAURI__.event.listen.mock.calls.find(([name]) => name === 'artifact-created')[1]
    listener({payload:{id}})
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_view',expect.objectContaining({label:'artifact',artifactId:id})))
  })
})

it('saves a homepage and uses it for Home and fresh native views', async () => {
  const view = render(BrowserWorkspace, {tauri:{invoke}})
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_view', expect.objectContaining({homepage:'https://muniment.ai/docs/'})))
  await fireEvent.click(screen.getByRole('button', {name:'Browser settings'}))
  await fireEvent.input(screen.getByLabelText('Homepage'), {target:{value:'https://example.net/start'}})
  await fireEvent.click(screen.getByRole('button', {name:'Save'}))
  await fireEvent.click(screen.getByRole('button', {name:'Home'}))
  expect(invoke).toHaveBeenCalledWith('browser_command', {request:{view:'browser',action:'navigate',value:'https://example.net/start'}})
  view.unmount()
  invoke.mockClear()
  render(BrowserWorkspace, {tauri:{invoke}})
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_view', expect.objectContaining({homepage:'https://example.net/start'})))
})
it('rejects unsupported homepage schemes', async () => {
  render(BrowserWorkspace, {tauri:{invoke}})
  await fireEvent.click(screen.getByRole('button', {name:'Browser settings'}))
  await fireEvent.input(screen.getByLabelText('Homepage'), {target:{value:'file:///tmp/private'}})
  await fireEvent.click(screen.getByRole('button', {name:'Save'}))
  expect(screen.getByRole('alert')).toHaveTextContent('HTTP or HTTPS')
})

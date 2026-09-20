import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import BrowserWorkspace from './BrowserWorkspace.svelte'
let invoke
beforeEach(() => {
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
  it('requires an explicit grant and offers stop after it succeeds', async () => {
    render(BrowserWorkspace, { tauri: { invoke }, onclose: vi.fn() })
    await fireEvent.click(screen.getByRole('button', { name: 'Allow agent control' }))
    expect(invoke).toHaveBeenCalledWith('browser_command', { request: { view: 'browser', action: 'grant', value: '' } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Stop agent control' }))
    expect(await screen.findByRole('button', { name: 'Allow agent control' })).toBeInTheDocument()
  })
  it('saves an artifact before opening its isolated preview', async () => {
    render(BrowserWorkspace, { tauri: { invoke }, artifacts: true, onclose: vi.fn() })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Counter' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Artifact HTML' }), { target: { value: '<h1>Counter</h1>' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save and preview' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('artifact_save', { id: null, name: 'Counter', html: '<h1>Counter</h1>' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('browser_view', expect.objectContaining({ label: 'artifact', artifactId: '01900000-0000-7000-8000-000000000001' })))
    expect(screen.getByRole('button', { name: 'Edit' })).toBeInTheDocument()
  })
})

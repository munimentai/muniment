// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import { historyMessages } from './lib/chat-state.js'

let App
let invoke
let chatListener
let dictationListener
let eventUnlisten
let pairingUnlisten
let dialogResult
let dragDropListener
let dragDropUnlisten
let homeStatus

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((listener) => {
      dragDropListener = listener
      return Promise.resolve(dragDropUnlisten)
    }),
  }),
}))

const snapshot = (groups = []) => ({
  snapshot_version: 2,
  subject: 'user-123',
  user_display_name: 'Alice',
  org_id: 'org-123',
  organization_display_name: 'Acme',
  role: 'owner',
  territory: 'us',
  groups,
})

const device = (device_id, overrides = {}) => ({
  device_id,
  client_id: 'muniment-desktop',
  client_role: 'desktop',
  platform: 'desktop',
  created_at: '2026-01-01T00:00:00Z',
  revoked_at: null,
  last_active_at: '2026-01-02T00:00:00Z',
  current: false,
  ...overrides,
})

beforeAll(async () => {
  HTMLElement.prototype.scrollTo = vi.fn()
  window.__TAURI__ = {
    core: { invoke: (command, ...args) => command === 'home_status'
      ? Promise.resolve(homeStatus)
      : invoke(command, ...args) },
    event: { listen: vi.fn((event, listener) => {
      if (event === 'chat-event') chatListener = listener
      if (event === 'dictation-event') dictationListener = listener
      return Promise.resolve(event === 'attach-pairing-requested' ? pairingUnlisten : eventUnlisten)
    }) },
  }
  window.__TAURI_INTERNALS__ = {
    invoke: (command) => command === 'plugin:dialog|open' ? Promise.resolve(dialogResult) : Promise.reject(new Error(`unexpected internal command: ${command}`)),
    transformCallback: vi.fn(),
  }
  App = (await import('./App.svelte')).default
})

beforeEach(() => {
  homeStatus = { configured: true, homePath: '/Documents/Muniment' }
  chatListener = undefined
  dictationListener = undefined
  eventUnlisten = vi.fn()
  pairingUnlisten = vi.fn()
  dragDropListener = undefined
  dragDropUnlisten = vi.fn()
  dialogResult = null
  invoke = vi.fn(async (command, payload) => {
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_history') return []
    if (command === 'chat_file_metadata') return { displayName: payload.path.split(/[\\/]/).pop(), byteLength: 1536 }
    if (command === 'auth_entitlement_snapshot') return snapshot()
    if (command === 'auth_devices') return []
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => cleanup())

describe('Home onboarding', () => {
  it('blocks the shell and confirms the displayed Documents default', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    expect(screen.queryByText('Sign in')).not.toBeInTheDocument()
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-confirm'))
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
  })

  it('surfaces a scaffold failure and lets the user choose again', async () => {
    homeStatus = { configured: false, homePath: '/read-only/Muniment' }
    dialogResult = '/Documents/Muniment'
    invoke.mockRejectedValue('Muniment Home could not be created.')
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    expect(await screen.findByRole('alert')).toHaveTextContent('Muniment Home could not be created.')
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    await waitFor(() => expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment'))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('cancels Home settings back to the configured workspace', async () => {
    render(App)
    await fireEvent.click(await screen.findByText('Home settings'))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    await fireEvent.click(screen.getByTestId('onboarding-cancel'))
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
  })
})

describe('voice dictation', () => {
  it('starts on primary pointer down and stops on release while preserving transcript', async () => {
    let stopping = false
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') {
        stopping = true
        return { state: 'running' }
      }
      if (command === 'dictation_status' && stopping) return { state: 'stopped' }
      if (command === 'dictation_polish') return 'Polished after.'
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Before' } })
    const voice = screen.getByRole('button', { name: 'Voice' })

    await fireEvent.pointerDown(voice, { button: 0, pointerId: 1 })
    expect(invoke).toHaveBeenCalledWith('dictation_start')
    dictationListener({ payload: { type: 'transcript', text: 'after' } })
    await fireEvent.pointerUp(voice, { button: 0, pointerId: 1 })

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_polish', { transcript: 'after' }))
    await waitFor(() => expect(composer).toHaveValue('Before Polished after.'))
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1)
  })

  it('keeps the verbatim capture editable and explains a polish failure', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') throw { category: 'localAiUnavailable', message: 'Local AI is unavailable.' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.input(composer, { target: { value: 'Keep' } })
    await fireEvent.click(voice)
    dictationListener({ payload: { type: 'transcript', text: 'verbatim words' } })
    await fireEvent.click(voice)

    expect(await screen.findByRole('alert')).toHaveTextContent('Polishing is unavailable. You can edit or send the captured text.')
    expect(composer).toHaveValue('Keep verbatim words')
    expect(composer).not.toHaveAttribute('readonly')
    expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled()
  })

  it('blocks racing actions and ignores a stale polish completion after unmount', async () => {
    let resolvePolish
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return new Promise((resolve) => { resolvePolish = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    const view = render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    dictationListener({ payload: { type: 'transcript', text: 'old capture' } })
    await fireEvent.click(voice)

    expect(await screen.findByRole('status')).toHaveTextContent('Polishing on this device…')
    expect(voice).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
    expect(composer).toHaveAttribute('readonly')
    view.unmount()
    resolvePolish('Stale replacement')
    await Promise.resolve()
  })

  it('cancels a primary pointer capture, restores the snapshot, and focuses the composer', async () => {
    let stopping = false
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') {
        stopping = true
        return { state: 'running' }
      }
      if (command === 'dictation_status' && stopping) return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Exact draft  ' } })
    const voice = screen.getByRole('button', { name: 'Voice' })

    await fireEvent.pointerDown(voice, { button: 0, pointerId: 7 })
    dictationListener({ payload: { type: 'transcript', text: 'temporary' } })
    await fireEvent.pointerCancel(voice, { pointerId: 7 })

    expect(composer).toHaveValue('Exact draft  ')
    expect(composer).toHaveFocus()
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_polish')).toHaveLength(0)
    dictationListener({ payload: { type: 'transcript', text: 'late' } })
    expect(composer).toHaveValue('Exact draft  ')
    await waitFor(() => expect(voice).toHaveAttribute('aria-pressed', 'false'))
  })

  it('ignores an out-of-order status after Escape and accepts a later capture', async () => {
    let resolveStaleStatus
    let starts = 0
    let stops = 0
    let statusCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') {
        starts += 1
        return { state: 'running' }
      }
      if (command === 'dictation_stop') {
        stops += 1
        return { state: 'running' }
      }
      if (command === 'dictation_status') {
        statusCalls += 1
        if (statusCalls === 1) return new Promise((resolve) => { resolveStaleStatus = resolve })
        return { state: 'stopped' }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Snapshot' } })
    const voice = screen.getByRole('button', { name: 'Voice' })

    await fireEvent.pointerDown(voice, { button: 0, pointerId: 1 })
    dictationListener({ payload: { type: 'transcript', text: 'temporary' } })
    await waitFor(() => expect(statusCalls).toBe(1))
    await fireEvent.keyDown(document, { key: 'Escape' })
    expect(composer).toHaveValue('Snapshot')
    expect(composer).toHaveFocus()

    dictationListener({ payload: { type: 'transcript', text: 'during stop' } })
    expect(composer).toHaveValue('Snapshot')
    await waitFor(() => expect(statusCalls).toBe(2))
    await waitFor(() => expect(voice).toHaveAttribute('aria-pressed', 'false'))
    resolveStaleStatus({ state: 'running' })
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(voice).toHaveAttribute('aria-pressed', 'false')
    expect(composer).toHaveValue('Snapshot')
    await fireEvent.pointerDown(voice, { button: 0, pointerId: 2 })
    expect(starts).toBe(2)
    expect(stops).toBe(1)
    dictationListener({ payload: { type: 'transcript', text: 'accepted' } })
    await waitFor(() => expect(composer).toHaveValue('Snapshot accepted'))
    await fireEvent.pointerUp(voice, { pointerId: 2 })
  })

  it.each([' ', 'Enter'])('supports a %s key hold without synthesized-click duplicates', async (key) => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const voice = await screen.findByRole('button', { name: 'Voice' })

    await fireEvent.keyDown(voice, { key })
    await fireEvent.keyDown(voice, { key, repeat: true })
    await fireEvent.keyUp(voice, { key })
    await fireEvent.click(voice)

    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
  })

  it('Escape restores the snapshot and rejects late cancelled transcripts before a later capture', async () => {
    let resolveStart
    let starts = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') {
        starts += 1
        if (starts === 1) return new Promise((resolve) => { resolveStart = resolve })
        return { state: 'running' }
      }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Exact draft  ' } })
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.pointerDown(voice, { button: 0, pointerId: 1 })
    dictationListener({ payload: { type: 'transcript', text: 'temporary' } })
    await fireEvent.keyDown(document, { key: 'Escape' })

    expect(composer).toHaveValue('Exact draft  ')
    expect(composer).toHaveFocus()
    dictationListener({ payload: { type: 'transcript', text: 'late' } })
    expect(composer).toHaveValue('Exact draft  ')

    resolveStart({ state: 'starting' })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_stop'))
    dictationListener({ payload: { type: 'transcript', text: 'later still' } })
    expect(composer).toHaveValue('Exact draft  ')

    await fireEvent.pointerDown(voice, { button: 0, pointerId: 2 })
    dictationListener({ payload: { type: 'transcript', text: 'new words' } })
    await waitFor(() => expect(composer).toHaveValue('Exact draft  new words'))
    await fireEvent.pointerUp(voice, { button: 0, pointerId: 2 })
  })

  it('starts, appends transcript to an editable draft, and stops', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_status') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Existing draft' } })
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)

    expect(voice).toHaveAttribute('aria-pressed', 'true')
    expect(screen.getByRole('status')).toHaveTextContent('Starting local dictation…')
    dictationListener({ payload: { type: 'transcript', text: 'spoken words' } })
    await waitFor(() => expect(composer).toHaveValue('Existing draft spoken words'))
    await fireEvent.input(composer, { target: { value: 'Edited transcript' } })
    await fireEvent.click(voice)

    expect(invoke).toHaveBeenCalledWith('dictation_start')
    expect(invoke).toHaveBeenCalledWith('dictation_stop')
    expect(voice).toHaveAttribute('aria-pressed', 'false')
    expect(composer).toHaveValue('Edited transcript')
  })

  it('keeps an active capture stoppable and prevents starting a chat', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Question' } })
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)

    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).toBeDisabled()
    expect(voice).toBeEnabled()
    await fireEvent.click(send)
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())

    await fireEvent.click(voice)
    expect(invoke).toHaveBeenCalledWith('dictation_stop')
    expect(voice).toHaveAttribute('aria-pressed', 'false')
    expect(send).toBeEnabled()
  })

  it.each([
    ['modelNotInstalled', 'The speech model is not installed.'],
    ['failed', 'Microphone capture failed.'],
  ])('renders a terminal %s message and becomes retryable', async (state, message) => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_status') return { state, category: 'redacted', message }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Keep this' } })
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)

    expect(await screen.findByRole('alert')).toHaveTextContent(message)
    expect(voice).toHaveAttribute('aria-pressed', 'false')
    expect(voice).toBeEnabled()
    expect(composer).toHaveValue('Keep this')
  })

  it('keeps capture active and stoppable when status IPC rejects', async () => {
    let statusCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_status') {
        statusCalls += 1
        throw 'Dictation status is temporarily unavailable.'
      }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const voice = await screen.findByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)

    expect(await screen.findByRole('alert')).toHaveTextContent('Dictation status is temporarily unavailable.')
    expect(statusCalls).toBeGreaterThan(0)
    expect(voice).toHaveAttribute('aria-pressed', 'true')
    expect(voice).toBeEnabled()

    await fireEvent.click(voice)
    expect(invoke).toHaveBeenCalledWith('dictation_stop')
    expect(voice).toHaveAttribute('aria-pressed', 'false')
  })

  it('keeps capture active and allows Stop to be retried when stop IPC rejects', async () => {
    let stopCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_status') return { state: 'running' }
      if (command === 'dictation_stop') {
        stopCalls += 1
        if (stopCalls === 1) throw 'Dictation could not be stopped.'
        return { state: 'stopped' }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const voice = await screen.findByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    await fireEvent.click(voice)

    expect(await screen.findByRole('alert')).toHaveTextContent('Dictation could not be stopped.')
    expect(voice).toHaveAttribute('aria-pressed', 'true')
    expect(voice).toBeEnabled()

    await fireEvent.click(voice)
    expect(stopCalls).toBe(2)
    expect(voice).toHaveAttribute('aria-pressed', 'false')
  })

  it('disables voice during an active chat and cleans up its listener and timer', async () => {
    let resolveSubmit
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_status') return { state: 'running' }
      if (command === 'chat_submit') return new Promise((resolve) => { resolveSubmit = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    const view = render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    view.unmount()
    expect(eventUnlisten).toHaveBeenCalledTimes(2)
    expect(pairingUnlisten).toHaveBeenCalledTimes(1)
    await new Promise((resolve) => setTimeout(resolve, 130))
    expect(invoke).not.toHaveBeenCalledWith('dictation_status')

    render(App)
    const nextComposer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(nextComposer, { target: { value: 'Question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(screen.getByRole('button', { name: 'Voice' })).toBeDisabled()
    resolveSubmit({ runId: 'run-1' })
  })
})

describe('local file selection', () => {
  it('shows and clears the native drop affordance, then de-duplicates dropped files', async () => {
    dialogResult = ['/private/contracts/lease.pdf']
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Add files' }))
    await waitFor(() => expect(dragDropListener).toBeDefined())

    dragDropListener({ payload: { type: 'over', position: { x: 10, y: 10 } } })
    expect(await screen.findByRole('status')).toHaveTextContent('Drop files to add themSelected locally · not sent to the model')
    dragDropListener({ payload: { type: 'leave' } })
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())

    dragDropListener({ payload: { type: 'over', position: { x: 20, y: 20 } } })
    dragDropListener({ payload: { type: 'drop', paths: ['/private/contracts/lease.pdf', '/private/notes.txt', '/private/notes.txt'] } })
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())
    expect(await screen.findByText('notes.txt')).toBeInTheDocument()
    expect(screen.getAllByText('lease.pdf')).toHaveLength(1)
    expect(screen.getAllByText('notes.txt')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_file_metadata')).toEqual([
      ['chat_file_metadata', { path: '/private/contracts/lease.pdf' }],
      ['chat_file_metadata', { path: '/private/notes.txt' }],
    ])
    expect(document.body).not.toHaveTextContent('/private/')
  })

  it('ignores native drag/drop while signed out and unregisters on teardown', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false }
      throw new Error(`unexpected command: ${command}`)
    })
    const view = render(App)
    await screen.findByRole('button', { name: 'Sign in' })
    await waitFor(() => expect(dragDropListener).toBeDefined())

    dragDropListener({ payload: { type: 'over', position: { x: 10, y: 10 } } })
    dragDropListener({ payload: { type: 'drop', paths: ['/secret/evidence.pdf'] } })
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('chat_file_metadata', expect.anything())

    view.unmount()
    expect(dragDropUnlisten).toHaveBeenCalledOnce()
  })

  it('treats picker cancel as a no-op and removes a selected file', async () => {
    render(App)
    const add = await screen.findByRole('button', { name: 'Add files' })
    await fireEvent.click(add)
    expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument()

    dialogResult = ['/private/contracts/lease.pdf', 'C:\\notes\\brief.txt']
    await fireEvent.click(add)
    expect(await screen.findByText('lease.pdf')).toBeInTheDocument()
    expect(screen.getByText('brief.txt')).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Remove lease.pdf' }))
    expect(screen.queryByText('lease.pdf')).not.toBeInTheDocument()
    expect(screen.getByText('brief.txt')).toBeInTheDocument()
  })

  it('retains draft and selection when local ingestion fails', async () => {
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'chat_file_metadata') return { displayName: 'evidence.pdf', byteLength: 2048 }
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') throw 'One or more selected files could not be added. Check the files and try again.'
      throw new Error(`unexpected command: ${command}`)
    })
    dialogResult = ['/secret/location/evidence.pdf']
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Add files' }))
    const composer = screen.getByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Review this' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    await screen.findByRole('alert')
    expect(composer).toHaveValue('Review this')
    expect(screen.getByText('evidence.pdf')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('chat_submit', {
      prompt: 'Review this', files: [{ path: '/secret/location/evidence.pdf' }],
    })
    expect(screen.queryByText('/secret/location/evidence.pdf')).not.toBeInTheDocument()
  })

  it('submits selected paths and clears the draft and selection only after success', async () => {
    let resolveSubmit
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'chat_file_metadata') {
        return { displayName: payload.path.split('/').pop(), byteLength: 1024 }
      }
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return new Promise((resolve) => { resolveSubmit = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    dialogResult = ['/private/contracts/lease.pdf', '/private/notes.txt']
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Add files' }))
    const composer = screen.getByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Review these' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    expect(invoke).toHaveBeenCalledWith('chat_submit', {
      prompt: 'Review these',
      files: [
        { path: '/private/contracts/lease.pdf' },
        { path: '/private/notes.txt' },
      ],
    })
    expect(composer).toHaveValue('Review these')
    expect(screen.getByText('lease.pdf')).toBeInTheDocument()
    expect(screen.getByText('notes.txt')).toBeInTheDocument()

    resolveSubmit({ runId: 'run-with-files', attachments: [
      { displayName: 'lease.pdf', byteLength: 1024 },
      { displayName: 'notes.txt', byteLength: 1024 },
    ] })
    await waitFor(() => expect(composer).toHaveValue(''))
    expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument()
    const saved = screen.getByRole('list', { name: 'Saved attachments' })
    expect(saved).toHaveTextContent('lease.pdf1.0 KBSaved locally · not sent to model')
    expect(document.body).not.toHaveTextContent('/private/contracts')
  })
})

it('hydrates safe durable attachment chips without paths or hashes', async () => {
  invoke.mockImplementation(async (command) => {
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_history') return [{
      runId: 'restored', prompt: 'Review it', phase: 'complete', text: 'Done', receipt: {},
      toolActivity: [], resumable: false,
      attachments: [{ displayName: 'contract.pdf', byteLength: 219136 }],
    }]
    if (command === 'auth_entitlement_snapshot') return snapshot()
    if (command === 'auth_devices') return []
    throw new Error(`unexpected command: ${command}`)
  })
  render(App)
  const saved = await screen.findByRole('list', { name: 'Saved attachments' })
  expect(saved).toHaveTextContent('contract.pdf214 KBSaved locally · not sent to model')
  expect(document.body).not.toHaveTextContent('/private/contract.pdf')
  expect(document.body).not.toHaveTextContent('sha256')
})

it('hydrates durable attachment chips when the prompt is unavailable', async () => {
  invoke.mockImplementation(async (command) => {
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_history') return [{
      runId: 'restored-without-prompt', prompt: null, phase: 'complete', text: 'Done', receipt: {},
      toolActivity: [], resumable: false,
      attachments: [{ displayName: 'evidence.txt', byteLength: 1536 }],
    }]
    if (command === 'auth_entitlement_snapshot') return snapshot()
    if (command === 'auth_devices') return []
    throw new Error(`unexpected command: ${command}`)
  })
  render(App)

  const saved = await screen.findByRole('list', { name: 'Saved attachments' })
  expect(saved).toHaveTextContent('evidence.txt1.5 KBSaved locally · not sent to model')
  expect(screen.getByText('Prompt unavailable')).toBeInTheDocument()
  expect(saved.closest('.user-message')).not.toBeNull()
  expect(document.body).not.toHaveTextContent('/private/evidence.txt')
  expect(document.body).not.toHaveTextContent('sha256')
})

describe('history hydration', () => {
  it('retains projected tool activity in the restored assistant run', () => {
    const toolActivity = [
      { effectId: 'tool-1', displayName: 'Search files', status: 'completed' },
    ]

    const messages = historyMessages([{
      runId: 'run-1',
      phase: 'complete',
      text: 'Done',
      prompt: 'Find it',
      receipt: null,
      toolActivity,
    }])

    expect(messages[1].run.toolActivity).toEqual(toolActivity)
  })

  it('defaults missing historical tool activity to an empty array', () => {
    const messages = historyMessages([{ runId: 'run-1', phase: 'complete', text: 'Done' }])

    expect(messages[0].run.toolActivity).toEqual([])
  })
})

describe('interrupted reply resume', () => {
  const interrupted = (resumable = true) => [{
    runId: 'run-interrupted', phase: 'interrupted', text: 'Partial answer',
    prompt: 'Original secret prompt', receipt: null, toolActivity: [], resumable,
  }]

  it('resumes the same run once and keeps its partial response visible', async () => {
    let resolveResume
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return interrupted()
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_resume') return new Promise((resolve) => { resolveResume = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const button = await screen.findByRole('button', { name: 'Resume' })
    await fireEvent.click(button)
    await fireEvent.click(button)
    expect(screen.getByText('Partial answer')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Resuming interrupted reply…')).toBeDisabled()
    expect(screen.queryByRole('button', { name: 'Queue follow-up' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Send' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Resuming…' })).toBeDisabled()
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_resume')).toEqual([
      ['chat_resume', { runId: 'run-interrupted' }],
    ])
    resolveResume({ runId: 'run-interrupted' })
  })

  it('does not regress live completion when resume invocation resolves later', async () => {
    let resolveResume
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return interrupted()
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_resume') return new Promise((resolve) => { resolveResume = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Resume' }))
    chatListener({ payload: {
      runId: 'run-interrupted', phase: 'complete', text: 'Finished answer',
      receipt: { id: 'receipt-1' }, toolActivity: [], pendingPermission: null,
    } })
    expect(await screen.findByText('Finished answer')).toBeInTheDocument()

    resolveResume({ runId: 'run-interrupted' })
    await waitFor(() => expect(screen.queryByText('Resuming…')).not.toBeInTheDocument())
    expect(screen.getByText('Finished answer')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Resume' })).not.toBeInTheDocument()
  })

  it('offers a new-run fallback only when interruption is not resumable', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return interrupted(false)
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByRole('button', { name: 'Try again' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Resume' })).not.toBeInTheDocument()
  })

  it('keeps a rejected resume interrupted with a retryable error', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return interrupted()
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_resume') throw 'This reply cannot be resumed.'
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Resume' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('This reply cannot be resumed.')
    expect(screen.getByRole('button', { name: 'Resume' })).toBeEnabled()
    expect(screen.getByText('Partial answer')).toBeInTheDocument()
  })
})

describe('tool activity cards', () => {
  const historyWith = (toolActivity, phase = 'complete') => [{
    runId: 'run-tools', phase, text: 'I used tools.', prompt: 'Do work', receipt: {}, toolActivity,
  }]

  function restore(toolActivity, phase) {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return historyWith(toolActivity, phase)
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    return render(App)
  }

  it('renders restored completed activity collapsed to its labeled header', async () => {
    restore([{ effectId: 'tool-1', displayName: 'Search files', status: 'completed' }])

    expect(await screen.findByRole('status', { name: 'Search files: completed' })).toHaveTextContent('Search filescompleted')
  })

  it('updates a live running card to completed', async () => {
    restore([{ effectId: 'tool-1', displayName: 'Read file', status: 'running' }], 'streaming')
    expect(await screen.findByRole('status', { name: 'Read file: running' })).toBeInTheDocument()
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))

    chatListener({ payload: {
      runId: 'run-tools', phase: 'complete', text: 'I used tools.', receipt: {},
      toolActivity: [{ effectId: 'tool-1', displayName: 'Read file', status: 'completed' }],
    } })

    expect(await screen.findByRole('status', { name: 'Read file: completed' })).toBeInTheDocument()
    expect(screen.queryByRole('status', { name: 'Read file: running' })).not.toBeInTheDocument()
  })

  it('labels failed activity with text and supplies a neutral missing name', async () => {
    restore([{ effectId: 'tool-1', displayName: null, status: 'failed' }])

    const card = await screen.findByRole('status', { name: 'Tool activity: failed' })
    expect(card).toHaveTextContent('Tool activity')
    expect(card).toHaveTextContent('failed')
  })

  it('groups parallel running effects and keeps every status row visible when settled', async () => {
    restore([
      { effectId: 'tool-1', displayName: 'Search files', status: 'running' },
      { effectId: 'tool-2', displayName: 'Read file', status: 'running' },
    ], 'streaming')

    const group = await screen.findByRole('group', { name: /Parallel tool activity: Search files running, Read file running/ })
    expect(within(group).getByLabelText('Search files: running')).toBeInTheDocument()
    expect(within(group).getByLabelText('Read file: running')).toBeInTheDocument()
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))

    chatListener({ payload: {
      runId: 'run-tools', phase: 'complete', text: 'I used tools.', receipt: {},
      toolActivity: [
        { effectId: 'tool-1', displayName: 'Search files', status: 'completed' },
        { effectId: 'tool-2', displayName: 'Read file', status: 'failed' },
      ],
    } })

    const settled = await screen.findByRole('group', { name: /Search files completed, Read file failed/ })
    expect(within(settled).getByLabelText('Search files: completed')).toBeInTheDocument()
    expect(within(settled).getByLabelText('Read file: failed')).toBeInTheDocument()
  })
})

describe('active run composer queue', () => {
  beforeEach(() => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return { runId: 'run-7' }
      if (command === 'chat_queue') return undefined
      throw new Error(`unexpected command: ${command}`)
    })
  })

  async function startRun() {
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Initial prompt' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    await screen.findByRole('button', { name: 'Queue follow-up' })
    return composer
  }

  function expectQueuePayload(payload) {
    const call = invoke.mock.calls.find(([command]) => command === 'chat_queue')
    // These are the camelCased flattened chat_queue parameters in src-tauri/src/chat.rs;
    // delivery spellings come from ChatDelivery's rename_all = "camelCase".
    expect(call?.[1]).toEqual(payload)
  }

  it('queues a follow-up with the exact Rust command payload', async () => {
    const composer = await startRun()
    await fireEvent.input(composer, { target: { value: 'Then summarize it' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Queue follow-up' }))

    expectQueuePayload({ runId: 'run-7', delivery: 'followUp', message: 'Then summarize it' })
  })

  it('steers the active reply with the exact Rust command payload', async () => {
    const composer = await startRun()
    await fireEvent.input(composer, { target: { value: 'Focus on the risks' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    expectQueuePayload({ runId: 'run-7', delivery: 'steer', message: 'Focus on the risks' })
  })

  it('shows a queue rejection and preserves the draft', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return { runId: 'run-7' }
      if (command === 'chat_queue') throw 'Could not queue this message'
      throw new Error(`unexpected command: ${command}`)
    })
    const composer = await startRun()
    await fireEvent.input(composer, { target: { value: 'Keep this draft' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Queue follow-up' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Could not queue this message')
    expect(composer).toHaveValue('Keep this draft')
  })
})

describe('signed-in access popover', () => {
  it('loads server-issued profile fields while entering the workspace', async () => {
    render(App)

    const profile = await screen.findByRole('button', { name: /Alice/i })
    expect(profile).toHaveTextContent('Acme · owner')
    expect(invoke).toHaveBeenCalledWith('auth_entitlement_snapshot')
  })

  it('clears the previous account profile before a new session access fetch fails', async () => {
    let accessCalls = 0
    let rejectNewSessionAccess
    const newSessionAccess = new Promise((_, reject) => { rejectNewSessionAccess = reject })
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'user-a' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') {
        accessCalls += 1
        if (accessCalls <= 2) return snapshot()
        return newSessionAccess
      }
      if (command === 'auth_devices') return []
      if (command === 'auth_sign_out') return { signed_in: false, subject: null }
      if (command === 'auth_sign_in') return { signed_in: true, subject: 'user-b' }
      throw new Error(`unexpected command: ${command}`)
    })

    render(App)
    const oldProfile = await screen.findByRole('button', { name: /Alice/i })
    await fireEvent.click(oldProfile)
    await fireEvent.click(screen.getByRole('button', { name: 'Sign out' }))
    await screen.findByRole('button', { name: 'Sign in' })

    await fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))
    const newProfile = await screen.findByRole('button', { name: /user-b/i })
    expect(newProfile).toHaveTextContent('Access unavailable')
    expect(screen.queryByText('Alice')).not.toBeInTheDocument()
    expect(screen.queryByText('Acme · owner')).not.toBeInTheDocument()

    expect(accessCalls).toBe(3)
    rejectNewSessionAccess(new Error('offline'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.queryByText('Alice')).not.toBeInTheDocument()
    expect(screen.queryByText('Acme · owner')).not.toBeInTheDocument()
  })

  it('keeps fetch states local, retries, expands duplicate groups independently, and closes accessibly', async () => {
    let rejectOpen
    const pendingOpen = new Promise((_, reject) => { rejectOpen = reject })
    let accessCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') {
        accessCalls += 1
        if (accessCalls === 1) return snapshot()
        if (accessCalls === 2) return pendingOpen
        return snapshot([
          { name: 'members', models: [], connections: [], capabilities: [] },
          { name: 'members', models: ['gpt'], connections: ['warehouse'], capabilities: ['chat'] },
        ])
      }
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })

    render(App)
    const profile = await screen.findByRole('button', { name: /Alice/i })
    await fireEvent.click(profile)

    const dialog = screen.getByRole('dialog', { name: 'Your access' })
    expect(within(dialog).getByText('Checking your current access…')).toBeInTheDocument()
    expect(profile).toHaveTextContent('Acme · owner')
    expect(screen.getByText(/Ask anything/)).toBeInTheDocument()

    rejectOpen(new Error('offline'))
    const retry = await within(dialog).findByRole('button', { name: 'Try again' })
    expect(screen.getByText(/Ask anything/)).toBeInTheDocument()
    await fireEvent.click(retry)
    expect(accessCalls).toBe(3)

    const toggles = await within(dialog).findAllByRole('button', { name: 'members' })
    expect(toggles).toHaveLength(2)
    expect(toggles[0]).toHaveAttribute('aria-expanded', 'false')
    expect(toggles[1]).toHaveAttribute('aria-expanded', 'false')
    await fireEvent.click(toggles[0])
    expect(toggles[0]).toHaveAttribute('aria-expanded', 'true')
    expect(toggles[1]).toHaveAttribute('aria-expanded', 'false')
    expect(within(dialog).getAllByText('None granted')).toHaveLength(3)

    document.body.focus()
    await fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByRole('dialog', { name: 'Your access' })).not.toBeInTheDocument()
    expect(profile).toHaveFocus()
    expect(profile).toHaveAttribute('aria-expanded', 'false')

    await fireEvent.click(profile)
    await screen.findByRole('dialog', { name: 'Your access' })
    await fireEvent.click(document.body)
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Your access' })).not.toBeInTheDocument())
    expect(profile).toHaveFocus()
  })

  it('renders mixed device states in deterministic order without disturbing groups', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot([{ name: 'members', models: ['gpt'], connections: [], capabilities: [] }])
      if (command === 'auth_devices') return [
        device('revoked-newest', { platform: 'ios', revoked_at: '2026-06-01T00:00:00Z', last_active_at: '2026-07-01T00:00:00Z' }),
        device('active-old', { platform: 'android', last_active_at: '2026-05-01T00:00:00Z' }),
        device('current-new', { current: true, last_active_at: '2026-06-01T00:00:00Z' }),
      ]
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Your access' })
    const rows = await within(dialog).findAllByRole('listitem')
    expect(rows.map((row) => row.textContent)).toEqual(expect.arrayContaining([
      expect.stringContaining('This device'), expect.stringContaining('Active'), expect.stringContaining('Revoked'),
    ]))
    expect(rows[0]).toHaveTextContent('desktopThis deviceActive')
    expect(rows[1]).toHaveTextContent('androidActive')
    expect(rows[2]).toHaveTextContent('iosRevoked')
    expect(within(dialog).getByRole('button', { name: 'members' })).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('revoked-newest')
  })

  it('renders an empty device response', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    expect(await screen.findByText('No devices found')).toBeInTheDocument()
  })

  it('retries only a failed device request and keeps entitlement groups rendered', async () => {
    let deviceCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot([{ name: 'members', models: [], connections: [], capabilities: [] }])
      if (command === 'auth_devices') {
        deviceCalls += 1
        if (deviceCalls === 1) throw new Error('raw backend secret')
        return [device('recovered')]
      }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Your access' })
    expect(await within(dialog).findByText('Devices could not be loaded.')).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('raw backend secret')
    expect(within(dialog).getByRole('button', { name: 'members' })).toBeInTheDocument()
    const entitlementCalls = invoke.mock.calls.filter(([command]) => command === 'auth_entitlement_snapshot').length
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Try again' }))
    expect(await within(dialog).findByText('Active')).toBeInTheDocument()
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_entitlement_snapshot')).toHaveLength(entitlementCalls)
  })
})

// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import { historyMessages } from './lib/chat-state.js'

let App
let invoke
let chatListener
let dialogResult

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
    core: { invoke: (...args) => invoke(...args) },
    event: { listen: vi.fn((_, listener) => { chatListener = listener; return Promise.resolve(vi.fn()) }) },
  }
  window.__TAURI_INTERNALS__ = {
    invoke: (command) => command === 'plugin:dialog|open' ? Promise.resolve(dialogResult) : Promise.reject(new Error(`unexpected internal command: ${command}`)),
    transformCallback: vi.fn(),
  }
  App = (await import('./App.svelte')).default
})

beforeEach(() => {
  chatListener = undefined
  dialogResult = null
  invoke = vi.fn(async (command) => {
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_history') return []
    if (command === 'auth_entitlement_snapshot') return snapshot()
    if (command === 'auth_devices') return []
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => cleanup())

describe('local file selection', () => {
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
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
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
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
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

    resolveSubmit({ runId: 'run-with-files' })
    await waitFor(() => expect(composer).toHaveValue(''))
    expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument()
  })
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

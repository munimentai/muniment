// @vitest-environment jsdom

import fs from 'node:fs'
import path from 'node:path'

import { tick } from 'svelte'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import { historyMessages } from './lib/chat-state.js'

const appSource = fs.readFileSync(path.join(process.cwd(), 'src/App.svelte'), 'utf8')
const appStyles = appSource.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const appRules = new Map([...appStyles
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))
const accountSettingsSource = fs.readFileSync(path.join(process.cwd(), 'src/lib/AccountSettings.svelte'), 'utf8')
const rowControlStyles = fs.readFileSync(path.join(process.cwd(), 'src/lib/RowControl.svelte'), 'utf8').match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const settingsStyles = fs.readFileSync(path.join(process.cwd(), 'src/lib/Settings.svelte'), 'utf8').match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const emptyInventory = { providers: [], default_provider: null, default_model: null, hidden: [] }
const ollamaInventory = { providers: [{ id: 'ollama', name: 'Ollama', source: 'local', base_url: 'http://localhost:11434/v1', models: [{ id: 'llama3.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false }] }], default_provider: 'ollama', default_model: 'llama3.2:3b', hidden: [] }
const keyInventory = { providers: [{ id: 'anthropic', name: 'Anthropic', source: 'key', base_url: null, models: [{ id: 'claude-sonnet-5', context: '1M', max_out: '128K', thinking: true, images: true }] }], default_provider: null, default_model: null, hidden: [] }
// Settings opens from the sidebar control; a section name picks that section's nav button.
const openSettings = async (section) => {
  await fireEvent.click(await screen.findByRole('button', { name: 'Settings' }))
  const dialog = await screen.findByRole('dialog', { name: 'Settings' })
  if (section) await fireEvent.click(within(within(dialog).getByRole('navigation', { name: 'Settings sections' })).getByRole('button', { name: section }))
  return dialog
}
const rowControlRules = new Map([...rowControlStyles
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))
// The delete action lives in the row's right-click menu and opens the inline confirmation.
const openDeleteMenu = async (title) => {
  const row = [...document.querySelectorAll('.thread-record .thread-row')].find((row) => row.querySelector('.thread-row-title').textContent === title)
  await fireEvent.contextMenu(row)
  await fireEvent.click(screen.getByRole('menuitem', { name: `Delete ${title}` }))
}
const modifiedCodeDiff = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'protocol-fixtures/code-diff/1/modified.json'), 'utf8'))

let App
let invoke
let chatListener
let launcherListener
let launcherReply
let dictationListener
let entitlementListener
let registrationRetryListener
let desktopClientListener
let desktopClientUnlisten
let desktopClientListen
let eventUnlisten
let pairingListener
let pairingUnlisten
let pairingRegistrationError
let dialogResult
let windowFocused
let requestUserAttention
let dragDropListener
let dragDropUnlisten
let dragDropRegistrationError
let homeStatus
let scanReport
let globalShortcutHandler
let registerGlobalShortcut
let unregisterGlobalShortcut
let registeredShortcuts
let threadSummaryResult
let olderThreadSummaryResult
let recordCompaniesResult
let recordKindsResult
let recordQueryResult
let recordEntityResult
let recordProposeResult
let recordCommitResult
let readerDescribeResult
let readerRunResults
let readerObjectsResult
let readerConnectResult
let localModeStatus
let runtimeState
let runtimeListener

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: (...args) => registerGlobalShortcut(...args),
  unregister: (...args) => unregisterGlobalShortcut(...args),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: () => Promise.resolve(dialogResult) }))

vi.mock('@tauri-apps/api/window', () => ({
  UserAttentionType: { Informational: 2 },
  getCurrentWindow: () => ({
    isFocused: () => Promise.resolve(windowFocused),
    requestUserAttention: (...args) => requestUserAttention(...args),
  }),
}))

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((listener) => {
      dragDropListener = listener
      return dragDropRegistrationError
        ? Promise.reject(dragDropRegistrationError)
        : Promise.resolve(dragDropUnlisten)
    }),
  }),
}))

const grant = (overrides = {}) => ({
  id: 'grt_stub', principal_type: 'org', principal_id: 'org-123',
  resource_type: 'model', resource_id: 'gpt', action: 'use', effect: 'allow',
  expires_at: null, ...overrides,
})

const snapshot = (grants = []) => ({
  snapshot_version: 2,
  subject: 'user-123',
  user_display_name: 'Alice',
  org_id: 'org-123',
  organization_display_name: 'Acme',
  role: 'owner',
  territory: 'us',
  capabilities: [],
  grants,
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

async function findWorkspaceComposer() {
  const composer = await screen.findByLabelText('Message', { selector: '#composer-message' })
  expect(composer).toHaveAttribute('placeholder', 'Ask anything')
  return composer
}

async function stopClickCapture(voice) {
  await fireEvent.click(voice)
  await fireEvent.click(voice)
}

function deferred() {
  let resolve
  let reject
  const promise = new Promise((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

beforeAll(async () => {
  HTMLElement.prototype.scrollTo = vi.fn()
  Element.prototype.animate = function animate(_keyframes, options = {}) {
    let timer
    const animation = {
      currentTime: options.duration ?? 0,
      effect: {},
      playState: 'running',
      cancel() {
        clearTimeout(timer)
        animation.playState = 'idle'
      },
    }
    Object.defineProperty(animation, 'onfinish', {
      set(finish) {
        timer = setTimeout(() => {
          animation.playState = 'finished'
          finish()
        }, options.duration ?? 0)
      },
    })
    return animation
  }
  window.__TAURI__ = {
    core: { invoke: (command, ...args) => {
      if (command === 'chat_thread_summaries') {
        if (args[0]?.cursor === 'older') return Promise.resolve({ summaries: olderThreadSummaryResult, nextCursor: null })
        return Promise.resolve({ summaries: threadSummaryResult, nextCursor: olderThreadSummaryResult ? 'older' : null })
      }
      if (command === 'chat_thread_open') {
        return Promise.resolve(invoke(command, ...args)).then((entries) => ({ entries, nextCursor: null }))
      }
      if (command === 'launcher_register') return Promise.resolve()
      if (command === 'runtime_state') return Promise.resolve(runtimeState)
      if (command === 'home_status') return Promise.resolve(homeStatus)
      if (command === 'onboarding_scan') return invoke(command).then(() => scanReport)
      if (command === 'local_mode_status') return Promise.resolve(localModeStatus)
      return invoke(command, ...args)
    } },
    event: { listen: vi.fn((event, listener) => {
      if (event === 'runtime-state-changed') runtimeListener = listener
      if (event === 'chat-event') chatListener = listener
      if (event === 'launcher-submit') launcherListener = listener
      if (event === 'dictation-event') dictationListener = listener
      if (event === 'entitlement-changed') entitlementListener = listener
      if (event === 'auth-registration-retry') registrationRetryListener = listener
      if (event === 'desktop-client-status-changed') desktopClientListener = listener
      if (event === 'attach-pairing-requested') pairingListener = listener
      if (event === 'attach-pairing-requested' && pairingRegistrationError) {
        return Promise.reject(pairingRegistrationError)
      }
      if (event === 'attach-pairing-requested') return Promise.resolve(pairingUnlisten)
      if (event === 'desktop-client-status-changed') return desktopClientListen(listener)
      return Promise.resolve(eventUnlisten)
    }), emitTo: (...args) => launcherReply(...args) },
  }
  window.__TAURI_INTERNALS__ = {
    invoke: (command) => command === 'plugin:dialog|open' ? Promise.resolve(dialogResult) : Promise.reject(new Error(`unexpected internal command: ${command}`)),
    transformCallback: vi.fn(),
  }
  App = (await import('./App.svelte')).default
})

beforeEach(() => {
  localStorage.clear()
  recordQueryResult = { page: { kind: 'deal', total: 1, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [{ id: 'deal-1', kind: 'deal', title: 'Northwind renewal', state: 'won', updated_at: '2026-09-15T10:30:00.000Z', created_at: '2026-09-15T10:00:00.000Z', body_text: 'Northwind renewal: won.', data: { name: 'Northwind renewal', stage: 'won' } }] } }
  recordEntityResult = { entity: { entity: { id: 'deal-1', kind: 'deal', title: 'Northwind renewal', state: 'won', updated_at: '2026-09-15T10:30:00.000Z', body_text: 'Northwind renewal: won.', data: { name: 'Northwind renewal', stage: 'won' } }, kind: { name: 'deal', schema: { properties: { name: {}, stage: {} }, stateProperty: 'stage' }, states: ['discovery', 'won'], extension: null }, identities: [{ kind: 'external', value: 'hubspot:deal:1', entity_id: 'deal-1' }], edges: [{ id: 'edge-1', relation: 'concerns', src_id: 'deal-1', dst_id: 'org-1', dst_title: 'Northwind', dst_kind: 'org', src_title: 'Northwind renewal', src_kind: 'deal', valid_from: '2026-09-15T10:00:00.000Z', valid_to: null }], events: [{ id: 'ev-1', seq: 3, at: '2026-09-15T10:00:00.000Z', verb: 'created', actor_id: 'owner-1', on_behalf_of: null }] } }
  recordProposeResult = { proposal: { id: 'proposal-1', warnings: ['Northwind Traders has an open deal closing 2026-09-16'], diff: { op: 'update', before: { data: { name: 'Northwind renewal', stage: 'won' } }, after: { data: { name: 'Northwind renewal FY27', stage: 'won' } } } } }
  recordCommitResult = { result: { event_seq: 4, entity_ids: ['deal-1'] } }
  readerDescribeResult = { description: { source: 'csv', object: '/exports/customers.csv', label: 'customers.csv', rows: 3, bytes: 120, hash: 'abc', fields: [
    { name: 'Company', guess: 'string', samples: ['Northwind', 'Contoso'], filled: 3 },
    { name: 'Website', guess: 'domain', samples: ['northwind.example'], filled: 2 },
    { name: 'Since', guess: 'date', samples: ['2020-01-15'], filled: 3 },
  ] } }
  readerObjectsResult = { error: { code: 'not_connected', message: 'Connect Stripe with its secret key first.' } }
  readerConnectResult = { connected: { source: 'stripe', label: 'Stripe', objects: [{ name: 'customers', label: 'Customers' }, { name: 'subscriptions', label: 'Subscriptions' }, { name: 'invoices', label: 'Invoices' }] } }
  readerRunResults = [{ run: { mapping: 'map-1', source: 'csv', object: '/exports/customers.csv', label: 'customers.csv', kind: 'org', offset: 0, next_offset: 3, done: true, total: 3, changed: true, created: 2, updated: 0, unchanged: 0, unplaced: 1, queued: 1, queue: [{ row: 3, title: 'No Site', reason: 'the Website cell is empty, so the row has no identity', cells: { Company: 'No Site', Website: '' } }] } }]
  recordCompaniesResult = { companies: [{ id: 'company-1', name: 'Northwind', created_at: '2026-01-01T00:00:00.000Z', owner_principal_id: 'owner-1', current: true }, { id: 'company-2', name: 'Surfoff', created_at: '2026-01-02T00:00:00.000Z', owner_principal_id: 'owner-2', current: false }], current: 'company-1' }
  recordKindsResult = { company_id: 'company-1', kinds: [{ name: 'person', schema: { properties: { full_name: {}, job_title: {} } }, states: null, extension: null }, { name: 'deal', schema: { properties: { name: {}, stage: {} }, stateProperty: 'stage' }, states: ['discovery', 'won'], extension: null }, { name: 'x_vendor', schema: { properties: { x_name: {} } }, states: null, extension: null }] }
  threadSummaryResult = [{ threadId: 'thread-1', title: '', updatedAt: '' }]
  olderThreadSummaryResult = null
  homeStatus = { configured: true, homePath: '/Documents/Muniment' }
  scanReport = { findings: [], errors: [] }
  localModeStatus = false
  runtimeState = { revision: 0, lastEvent: 'connected', visible: false, busy: false }
  runtimeListener = undefined
  chatListener = undefined
  launcherListener = undefined
  launcherReply = vi.fn().mockResolvedValue(undefined)
  dictationListener = undefined
  entitlementListener = undefined
  registrationRetryListener = undefined
  desktopClientListener = undefined
  desktopClientUnlisten = vi.fn()
  desktopClientListen = vi.fn().mockResolvedValue(desktopClientUnlisten)
  eventUnlisten = vi.fn()
  pairingListener = undefined
  pairingUnlisten = vi.fn()
  pairingRegistrationError = undefined
  windowFocused = true
  requestUserAttention = vi.fn().mockResolvedValue(undefined)
  dragDropListener = undefined
  dragDropUnlisten = vi.fn()
  dragDropRegistrationError = undefined
  dialogResult = null
  globalShortcutHandler = undefined
  registeredShortcuts = new Set()
  registerGlobalShortcut = vi.fn(async (shortcut, handler) => {
    registeredShortcuts.add(shortcut)
    globalShortcutHandler = handler
  })
  unregisterGlobalShortcut = vi.fn(async (shortcut) => { registeredShortcuts.delete(shortcut) })
  invoke = vi.fn(async (command, payload) => {
    if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'onboarding_scan') return
    if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
    if (command === 'onboarding_model_settings_error') return
    if (command === 'chat_thread_open') return []
    if (command === 'chat_file_metadata') return { displayName: payload.path.split(/[\\/]/).pop(), byteLength: 1536 }
    if (command === 'auth_entitlement_snapshot') return snapshot()
    if (command === 'auth_devices') return []
    if (command === 'attach_companions') return []
    if (command === 'attach_listener_status') return { started: true, failure: null }
    if (command === 'record_companies') return recordCompaniesResult
    if (command === 'installed_fonts') return ['Avenir Next', 'Inter', 'Iosevka']
    if (command === 'record_kinds') return recordKindsResult
    if (command === 'record_report') return { report: { company_id: 'company-1', open: 0, moved: null, findings: [], kinds: [] } }
    if (command === 'record_company_create') {
      recordCompaniesResult = { companies: [{ id: 'company-1', name: payload.name, created_at: '2026-01-01T00:00:00.000Z', owner_principal_id: 'owner-1', current: true }], current: 'company-1' }
      return { company: recordCompaniesResult.companies[0] }
    }
    if (command === 'record_company_select') return { company: { id: payload.companyId, current: true } }
    if (command === 'record_company_rename') {
      recordCompaniesResult = { ...recordCompaniesResult, companies: recordCompaniesResult.companies.map((company) => (company.id === payload.companyId ? { ...company, name: payload.name } : company)) }
      return { company: recordCompaniesResult.companies.find((company) => company.id === payload.companyId) }
    }
    if (command === 'record_query') return recordQueryResult
    if (command === 'record_entity') return recordEntityResult
    if (command === 'record_propose') return recordProposeResult
    if (command === 'record_commit') return recordCommitResult
    if (command === 'reader_describe') return readerDescribeResult
    if (command === 'reader_objects') return readerObjectsResult
    if (command === 'reader_connect') return readerConnectResult
    if (command === 'reader_run') return readerRunResults.length > 1 ? readerRunResults.shift() : readerRunResults[0]
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('launcher requests', () => {
  it('starts a fresh thread and preserves the main composer draft', async () => {
    const fallback = invoke.getMockImplementation()
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'chat_new_thread') return
      if (command === 'chat_submit') return { runId: 'launcher-run' }
      return fallback(command, payload)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'Keep my main draft' } })
    await launcherListener({ payload: { id: 'launcher-request', text: 'Start here' } })
    await waitFor(() => expect(screen.getByText('Start here', { selector: '.user-message p' })).toBeInTheDocument())
    expect(invoke).toHaveBeenCalledWith('chat_new_thread')
    expect(invoke).toHaveBeenCalledWith('chat_submit', { prompt: 'Start here', files: [] })
    expect(composer.value).toBe('Keep my main draft')
    expect(launcherReply).toHaveBeenCalledWith('launcher', 'launcher-result', { id: 'launcher-request', error: '' })
  })
})

describe('onboarding window layout', () => {
  it('bounds active onboarding without changing the other main layouts', () => {
    expect(appSource).toMatch(/<main class:onboarding-active=\{tauri && onboarding\.name !== 'complete'\}>/)
    expect(appRules.get('main.onboarding-active')).toMatch(/height:\s*100vh/)
    expect(appRules.get('main.onboarding-active')).toMatch(/grid-template-rows:\s*auto auto minmax\(0,\s*1fr\)/)
    expect(appRules.get('main.onboarding-active')).toMatch(/box-sizing:\s*border-box/)
  })
})

describe('entitlement change toast', () => {
  const copy = 'Your access changed. Some models or connections may differ.'

  it('is absent until the entitlement listener emits, then appears in its own status region', async () => {
    render(App)
    await screen.findByPlaceholderText('Ask anything')

    expect(screen.queryByText(copy)).not.toBeInTheDocument()
    entitlementListener({ payload: { snapshot_version: 3 } })

    expect(await screen.findByText(copy)).toHaveAttribute('role', 'status')
    expect(screen.getByTestId('run-announcement')).not.toHaveTextContent(copy)
  })

  it('clears a visible toast when the user signs out', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'auth_sign_out') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter') return undefined
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const profile = await screen.findByRole('button', { name: /Alice/i })
    entitlementListener({ payload: { snapshot_version: 3 } })
    expect(await screen.findByText(copy)).toBeInTheDocument()

    await fireEvent.click(profile)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Sign out' }))

    // A sign-out lands in local mode.
    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('local_mode_enter')
    expect(screen.queryByText(copy)).not.toBeInTheDocument()
  })
})

describe('pairing decisions', () => {
  it('records a rejected listener registration without a stale unlisten handle', async () => {
    pairingRegistrationError = new Error('sensitive registration detail')
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    const view = render(App)

    await waitFor(() => expect(error).toHaveBeenCalledWith('Pairing decision failed.'))
    view.unmount()

    expect(pairingUnlisten).not.toHaveBeenCalled()
    expect(error).not.toHaveBeenCalledWith(expect.stringContaining('sensitive'))
  })

  it.each([
    ['allows', 'Allow', true],
    ['denies', 'Deny', false],
  ])('%s a pairing request', async (_, button, approve) => {
    render(App)
    await waitFor(() => expect(pairingListener).toBeDefined())

    pairingListener({
      payload: {
        challenge: 'challenge-1',
        claimed_kind: 'ACP adapter',
        claimed_version: '2.4.1',
        workspace: 'Legal matters',
        scopes: ['run.write', 'thread.read'],
      },
    })

    const dialog = await screen.findByRole('dialog', { name: 'Approve Muniment connection' })
    expect(dialog).toHaveTextContent('The connecting program supplied these claims: kind ACP adapter and version 2.4.1.')
    expect(dialog).toHaveTextContent('Allow this program to access workspace Legal matters with the scopes run.write and thread.read?')
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Deny' })).toHaveFocus())

    await fireEvent.click(within(dialog).getByRole('button', { name: button }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_pairing_decide', {
      challenge: 'challenge-1', approve,
    }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it.each([
    ['strips control characters from', 'Legal\u0000 matters', 'workspace Legal matters'],
    ['bounds', `Legal matters${'x'.repeat(80)}`, 'workspace unknown'],
  ])('%s the workspace', async (_, workspace, copy) => {
    render(App)
    await waitFor(() => expect(pairingListener).toBeDefined())

    pairingListener({
      payload: {
        challenge: 'challenge-workspace',
        workspace,
        scopes: ['thread.read', 'run.write'],
      },
    })

    expect(await screen.findByRole('dialog')).toHaveTextContent(copy)
  })

  it('handles a rejected pairing decision without exposing its details', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'attach_pairing_decide') throw new Error('sensitive pairing detail')
      throw new Error(`unexpected command: ${command}`)
    })
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    render(App)
    await waitFor(() => expect(pairingListener).toBeDefined())

    pairingListener({ payload: { challenge: 'challenge-2' } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Allow' }))

    await waitFor(() => expect(error).toHaveBeenCalledWith('Pairing decision failed.'))
    expect(error).not.toHaveBeenCalledWith(expect.stringContaining('sensitive'))
  })

  it('denies with Escape and restores focus', async () => {
    render(App)
    const composer = await findWorkspaceComposer()
    composer.focus()
    await waitFor(() => expect(pairingListener).toBeDefined())

    pairingListener({ payload: { challenge: 'challenge-3' } })
    await screen.findByRole('dialog')
    await fireEvent.keyDown(document, { key: 'Escape' })

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_pairing_decide', {
      challenge: 'challenge-3', approve: false,
    }))
    await waitFor(() => expect(composer).toHaveFocus())
  })

  it.each([
    ['forward', 'Allow', false, 'Deny'],
    ['reverse', 'Deny', true, 'Allow'],
  ])('keeps %s Tab movement inside the pairing dialog', async (_, start, shiftKey, destination) => {
    render(App)
    await waitFor(() => expect(pairingListener).toBeDefined())

    pairingListener({ payload: { challenge: 'challenge-focus' } })
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Deny' })).toHaveFocus())
    const startButton = within(dialog).getByRole('button', { name: start })
    startButton.focus()

    await fireEvent.keyDown(document, { key: 'Tab', shiftKey })

    expect(within(dialog).getByRole('button', { name: destination })).toHaveFocus()
  })

  it.each([
    ['missing', {}],
    ['empty', { claimed_kind: '', claimed_version: '   ' }],
    ['over-long', { claimed_kind: 'x'.repeat(81), claimed_version: '1.0.0' }],
    ['control-character', { claimed_kind: 'ACP\u0000adapter', claimed_version: '1.0.0' }],
  ])('uses a neutral fallback for a %s claim', async (_, claim) => {
    render(App)
    await waitFor(() => expect(pairingListener).toBeDefined())

    pairingListener({
      payload: { challenge: 'challenge-hostile', ...claim },
    })

    expect(await screen.findByRole('dialog')).toHaveTextContent('unknown')
    await fireEvent.click(screen.getByRole('button', { name: 'Deny' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_pairing_decide', {
      challenge: 'challenge-hostile',
      approve: false,
    }))
  })

  it('queues a second request and requests attention for an unfocused window', async () => {
    windowFocused = false
    render(App)
    await waitFor(() => expect(pairingListener).toBeDefined())

    pairingListener({ payload: { challenge: 'first', claimed_kind: 'CLI', claimed_version: '1' } })
    pairingListener({ payload: { challenge: 'second', claimed_kind: 'Editor', claimed_version: '2' } })

    expect(await screen.findByRole('dialog')).toHaveTextContent('kind CLI and version 1')
    await waitFor(() => expect(requestUserAttention).toHaveBeenCalledTimes(2))
    await fireEvent.click(screen.getByRole('button', { name: 'Allow' }))
    await waitFor(() => expect(screen.getByRole('dialog')).toHaveTextContent('kind Editor and version 2'))
    await fireEvent.click(screen.getByRole('button', { name: 'Deny' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_pairing_decide', {
      challenge: 'second', approve: false,
    }))
  })
})

describe('workspace composer entry', () => {
  it.each([false, true])('shows the owner notice with Home configured as %s', async (configured) => {
    homeStatus = { configured, homePath: '/Documents/Muniment' }
    runtimeState = { revision: 1, lastEvent: 'exited', visible: true, busy: true }
    render(App)
    const notice = await screen.findByTestId('runtime-notice')
    expect(within(notice).getByText('The runtime exited.')).toHaveClass('record', 'error-record')
    expect(within(notice).getAllByRole('button')).toHaveLength(1)
    expect(within(notice).getByRole('button', { name: 'Start runtime' })).toBeDisabled()
    if (configured) {
      expect(screen.queryByRole('textbox', { name: 'Message' })).not.toBeInTheDocument()
    } else {
      const firstRun = await screen.findByRole('region', { name: 'First run' })
      expect(within(firstRun).getByRole('textbox', { name: 'Message' })).toBeVisible()
      expect(within(firstRun).getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
      expect(within(firstRun).getByTestId('onboarding-model')).toBeVisible()
      expect(within(firstRun).getByTestId('onboarding-scan')).toBeVisible()
      expect(within(firstRun).getByRole('button', { name: 'Send' })).toBeEnabled()
      expect(firstRun.parentElement).toBe(notice.parentElement)
    }
  })

  it('keeps the first-run draft and chips beside a start failure and after recovery', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    runtimeState = { revision: 1, lastEvent: 'starting', visible: false, busy: true }
    render(App)
    const firstRun = await screen.findByRole('region', { name: 'First run' })
    const composer = within(firstRun).getByRole('textbox', { name: 'Message' })
    await fireEvent.input(composer, { target: { value: 'Keep this first message' } })
    expect(screen.queryByTestId('runtime-notice')).not.toBeInTheDocument()

    runtimeListener({ payload: { revision: 2, lastEvent: 'startFailed', visible: true, busy: false,
      cause: 'The runtime start timed out. Desktop client connected: true. Chat events connected: false.' } })
    const notice = await screen.findByTestId('runtime-notice')
    expect(within(notice).getByText('The runtime start failed. The runtime start timed out. Desktop client connected: true. Chat events connected: false.')).toBeVisible()
    expect(firstRun).toBeVisible()
    expect(composer).toHaveValue('Keep this first message')
    for (const chip of ['onboarding-model', 'onboarding-home-path', 'onboarding-scan']) {
      expect(within(firstRun).getByTestId(chip)).toBeVisible()
    }
    await fireEvent.click(within(firstRun).getByTestId('onboarding-home-path'))
    expect(within(firstRun).getByRole('heading', { name: 'Home' })).toBeVisible()
    expect(within(firstRun).getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled', 'true')

    runtimeListener({ payload: { revision: 3, lastEvent: 'connected', visible: false, busy: false, cause: null } })
    await waitFor(() => expect(screen.queryByTestId('runtime-notice')).not.toBeInTheDocument())
    expect(composer).toHaveValue('Keep this first message')
    expect(firstRun).toBeVisible()
  })

  it.each([
    ['connected', false],
    ['requiresApproval', true],
    ['notFound', false],
    ['registrationFailed', false],
    ['requires_approval', false],
    [null, false],
  ])('offers Login Items only for the approval event %s', async (activation, visible) => {
    runtimeState = { revision: 1, lastEvent: activation, visible: activation !== 'connected', busy: false }
    vi.spyOn(window.navigator, 'userAgent', 'get').mockReturnValue('Macintosh')
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'attach_listener_status') return { connected: true, supervisor_running: true }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return []
      if (command === 'open_login_items') return undefined
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    if (['requiresApproval', 'notFound', 'registrationFailed'].includes(activation)) {
      await screen.findByTestId('runtime-notice')
    } else {
      await findWorkspaceComposer()
    }

    const button = screen.queryByRole('button', { name: 'Open Login Items' })
    expect(Boolean(button)).toBe(visible)
    if (button) {
      await fireEvent.click(button)
      expect(invoke).toHaveBeenCalledWith('open_login_items')
    }
  })

  it.each([false, true])('shows the queued session and clears it after recovery with Home configured as %s', async (configured) => {
    homeStatus = { configured, homePath: '/Documents/Muniment' }
    const cause = 'Task Scheduler kept the runtime task in state Queued for session id 7.'
    runtimeState = { revision: 1, lastEvent: 'startFailed', visible: true, busy: false, cause }
    render(App)

    const notice = await screen.findByTestId('runtime-notice')
    expect(within(notice).getByText(`The runtime start failed. ${cause}`)).toBeVisible()
    expect(within(notice).getByRole('button', { name: 'Start runtime' })).toBeEnabled()
    expect(screen.queryByRole('button', { name: 'Open Login Items' })).not.toBeInTheDocument()

    runtimeListener({ payload: { ...runtimeState, revision: 2, busy: true } })
    await waitFor(() => expect(within(notice).getByRole('button', { name: 'Start runtime' })).toBeDisabled())
    expect(within(notice).getByText(`The runtime start failed. ${cause}`)).toBeVisible()

    runtimeState = { revision: 3, lastEvent: 'connected', visible: false, busy: false, cause: null }
    runtimeListener({ payload: runtimeState })
    await waitFor(() => expect(screen.queryByTestId('runtime-notice')).not.toBeInTheDocument())
    expect(await screen.findByRole('textbox', { name: 'Message' })).toBeVisible()

    runtimeListener({ payload: { revision: 4, lastEvent: 'disconnected', visible: true, busy: false, cause: null } })
    const disconnected = await screen.findByTestId('runtime-notice')
    expect(within(disconnected).getByText('The runtime connection closed.')).toBeVisible()
    expect(disconnected).not.toHaveTextContent(cause)
  })

  it('reads the owner state outside macOS without an approval control', async () => {
    runtimeState = { revision: 1, lastEvent: 'startFailed', visible: true }
    render(App)
    expect(await screen.findByText('The runtime start failed.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Start runtime' })).toBeEnabled()
    expect(screen.queryByRole('button', { name: 'Open Login Items' })).not.toBeInTheDocument()
  })

  it('shows the background service notice after the boot status read fails and reads status after recovery', async () => {
    runtimeState = { revision: 1, lastEvent: 'disconnected', visible: true }
    let statusReads = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') {
        statusReads += 1
        if (statusReads === 1) throw new Error('Muniment cannot reach its background service.')
        return { signed_in: true, subject: 'token-subject' }
      }
      if (command === 'attach_listener_status') return { connected: false, supervisor_running: true }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'chat_thread_summaries') return { summaries: [], nextCursor: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    expect(await screen.findByText('The runtime connection closed.')).toBeInTheDocument()
    expect(screen.queryByText('Sign in to continue to your workspace.')).not.toBeInTheDocument()

    runtimeListener({ payload: { revision: 2, lastEvent: 'connected', visible: false } })
    desktopClientListener({ payload: { connected: true, supervisor_running: true } })
    expect(await screen.findByRole('textbox', { name: 'Message' })).toBeInTheDocument()
    expect(statusReads).toBe(2)
  })

  it('reads status when the initial listener status reports recovery without an event', async () => {
    let statusReads = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') {
        statusReads += 1
        if (statusReads === 1) throw new Error('Muniment cannot reach its background service.')
        return { signed_in: true, subject: 'token-subject' }
      }
      if (command === 'attach_listener_status') return { connected: true, supervisor_running: true }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'chat_thread_summaries') return { summaries: [], nextCursor: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    expect(await findWorkspaceComposer()).toBeInTheDocument()
    expect(statusReads).toBe(2)
  })

  it('keeps the recovered status when the failed boot read finishes late', async () => {
    const bootStatus = deferred()
    const listenerStatus = deferred()
    let statusReads = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') {
        statusReads += 1
        if (statusReads === 1) return bootStatus.promise
        return { signed_in: true, subject: 'token-subject' }
      }
      if (command === 'attach_listener_status') return listenerStatus.promise
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'chat_thread_summaries') return { summaries: [], nextCursor: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await waitFor(() => expect(desktopClientListener).toBeTypeOf('function'))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_listener_status'))

    desktopClientListener({ payload: { connected: true, supervisor_running: true } })
    expect(await screen.findByRole('textbox', { name: 'Message' })).toBeInTheDocument()
    bootStatus.reject(new Error('Muniment cannot reach its background service.'))
    listenerStatus.resolve({ connected: false, supervisor_running: true })

    await waitFor(() => expect(statusReads).toBe(2))
    expect(screen.getByRole('textbox', { name: 'Message' })).toBeInTheDocument()
  })

  it('shows the owner notice and repeats the start after a connection closes', async () => {
    runtimeState = { revision: 1, lastEvent: 'disconnected', visible: true }
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'attach_listener_status') return { connected: false, supervisor_running: true }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'chat_thread_summaries') return { summaries: [], nextCursor: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    expect(await screen.findByText('The runtime connection closed.')).toHaveClass('record', 'error-record')
    const notice = screen.getByTestId('runtime-notice')
    expect(within(notice).getAllByRole('button')).toHaveLength(1)
    invoke.mockResolvedValueOnce(undefined)
    await fireEvent.click(within(notice).getByRole('button', { name: 'Start runtime' }))
    expect(invoke).toHaveBeenCalledWith('runtime_start')
    expect(screen.queryByRole('textbox', { name: 'Message' })).not.toBeInTheDocument()

    runtimeListener({ payload: { revision: 2, lastEvent: 'connected', visible: false } })
    desktopClientListener({ payload: { connected: true, supervisor_running: true } })
    expect(await screen.findByRole('textbox', { name: 'Message' })).toBeInTheDocument()
    expect(screen.queryByText('The runtime connection closed.')).not.toBeInTheDocument()
  })

  it('shows the background service notice while the chat-event connection is down', async () => {
    runtimeState = { revision: 1, lastEvent: 'disconnected', visible: true }
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'attach_listener_status') {
        return { connected: true, chat_events_connected: false, supervisor_running: true }
      }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'chat_thread_summaries') return { summaries: [], nextCursor: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    expect(await screen.findByText('The runtime connection closed.')).toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: 'Message' })).not.toBeInTheDocument()
  })

  it('keeps the workspace mounted through a drop shorter than the notice dwell', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    render(App)
    const composer = await findWorkspaceComposer()

    runtimeListener({ payload: { revision: 1, lastEvent: 'disconnected', visible: false } })
    desktopClientListener({ payload: { connected: true, chat_events_connected: false, supervisor_running: true } })
    await vi.advanceTimersByTimeAsync(250)
    expect(screen.queryByText('The runtime connection closed.')).not.toBeInTheDocument()

    runtimeListener({ payload: { revision: 2, lastEvent: 'connected', visible: false } })
    desktopClientListener({ payload: { connected: true, chat_events_connected: true, supervisor_running: true } })
    await vi.advanceTimersByTimeAsync(5_000)
    expect(screen.queryByText('The runtime connection closed.')).not.toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Message' })).toBe(composer)
  })

  const chatEventsStatus = (chatEventsConnected) => ({
    connected: true, chat_events_connected: chatEventsConnected, supervisor_running: true,
  })

  function mockChatEventsStatus(chatEventsConnected) {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'attach_listener_status') return chatEventsStatus(chatEventsConnected)
      if (command === 'chat_thread_open') return []
      if (command === 'chat_current_thread') return 'thread-1'
      if (command === 'auth_entitlement_snapshot') return snapshot()
      throw new Error(`unexpected command: ${command}`)
    })
  }

  const threadOpens = () => invoke.mock.calls.filter(([command]) => command === 'chat_thread_open').length

  it('re-reads the open thread in place after the chat-event connection recovers', async () => {
    mockChatEventsStatus(true)
    render(App)
    await findWorkspaceComposer()
    invoke.mockClear()

    desktopClientListener({ payload: chatEventsStatus(false) })
    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', expect.anything())

    desktopClientListener({ payload: chatEventsStatus(true) })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', { threadId: 'thread-1', limit: 100 }))
    expect(threadOpens()).toBe(1)
    expect(invoke).toHaveBeenCalledWith('chat_current_thread')
    expect(invoke).not.toHaveBeenCalledWith('chat_select_thread', expect.anything())
  })

  it.each(['chat', 'request'])('restores a local reply when the %s socket reconnects first', async (firstSocket) => {
    localModeStatus = true
    let requestConnected = true
    let restored = false
    invoke.mockImplementation(async (command) => {
      if (command === 'attach_listener_status') return chatEventsStatus(true)
      if (command === 'chat_current_thread') {
        if (!requestConnected) throw new Error('The request socket closed.')
        return 'thread-1'
      }
      if (command === 'chat_thread_open') {
        if (!requestConnected) throw new Error('The request socket closed.')
        return [{ runId: 'run-1', prompt: 'Hello', text: restored ? 'The journal kept the reply.' : '', phase: restored ? 'complete' : 'streaming', receipt: {} }]
      }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await findWorkspaceComposer()
    await waitFor(() => expect(document.querySelector('.streaming')).toBeInTheDocument())
    requestConnected = false
    desktopClientListener({ payload: { ...chatEventsStatus(false), connected: false } })
    restored = true
    invoke.mockClear()
    if (firstSocket === 'chat') {
      desktopClientListener({ payload: { ...chatEventsStatus(true), connected: false } })
      await screen.findByText('Muniment could not restore conversation history. The request socket closed.')
      requestConnected = true
      desktopClientListener({ payload: chatEventsStatus(true) })
    } else {
      requestConnected = true
      desktopClientListener({ payload: chatEventsStatus(false) })
      await screen.findByText('The journal kept the reply.')
      desktopClientListener({ payload: chatEventsStatus(true) })
    }
    expect(await screen.findByText('The journal kept the reply.')).toBeInTheDocument()
    await waitFor(() => expect(document.querySelector('.streaming')).not.toBeInTheDocument())
    expect(invoke).not.toHaveBeenCalledWith('auth_status')
    expect(invoke).not.toHaveBeenCalledWith('chat_select_thread', expect.anything())
  })

  it('re-reads the open thread when the status poll reports the recovery', async () => {
    let finishRegistration
    desktopClientListen = vi.fn(() => new Promise((resolve) => { finishRegistration = resolve }))
    mockChatEventsStatus(true)
    render(App)
    await waitFor(() => expect(desktopClientListener).toBeDefined())
    await waitFor(() => expect(threadOpens()).toBe(1))
    await new Promise((resolve) => setTimeout(resolve, 0))

    // The first status reaches the window through the listener, so the poll
    // that follows carries the recovery.
    desktopClientListener({ payload: chatEventsStatus(false) })
    await waitFor(() => expect(threadOpens()).toBe(2))
    await new Promise((resolve) => setTimeout(resolve, 0))
    invoke.mockClear()

    finishRegistration(desktopClientUnlisten)

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_thread_open', { threadId: 'thread-1', limit: 100 }))
    expect(threadOpens()).toBe(1)
    expect(invoke).toHaveBeenCalledWith('chat_current_thread')
    expect(invoke).not.toHaveBeenCalledWith('chat_select_thread', expect.anything())
  })

  it('re-reads no thread on the first status the window reads', async () => {
    let finishRegistration
    desktopClientListen = vi.fn(() => new Promise((resolve) => { finishRegistration = resolve }))
    mockChatEventsStatus(true)
    render(App)
    await waitFor(() => expect(desktopClientListener).toBeDefined())
    await waitFor(() => expect(threadOpens()).toBe(1))
    await new Promise((resolve) => setTimeout(resolve, 0))
    invoke.mockClear()

    // The first status the window reads arrives after the thread opens, so a
    // re-read would show up as a second read.
    finishRegistration(desktopClientUnlisten)

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_listener_status'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    await new Promise((resolve) => setTimeout(resolve, 0))
    // The one read comes from the connected recovery, which calls run('status').
    expect(threadOpens()).toBe(1)
    expect(invoke).not.toHaveBeenCalledWith('chat_current_thread')
  })

  it('re-reads no thread while a status keeps the chat-event connection up', async () => {
    mockChatEventsStatus(true)
    render(App)
    await findWorkspaceComposer()
    invoke.mockClear()

    desktopClientListener({ payload: chatEventsStatus(true) })
    desktopClientListener({ payload: chatEventsStatus(true) })

    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_current_thread')
  })

  it('shows a thread added to the newest page during the chat-event drop', async () => {
    mockChatEventsStatus(true)
    render(App)
    await screen.findByPlaceholderText('Ask anything')
    await waitFor(() => expect(screen.getByRole('button', { name: 'New thread' })).toBeEnabled())
    await waitFor(() => expect(document.querySelector('.thread-row[aria-current="true"]')).toBeInTheDocument())
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(document.querySelector('[data-thread-id="thread-2"]')).not.toBeInTheDocument()
    invoke.mockClear()

    desktopClientListener({ payload: chatEventsStatus(false) })
    threadSummaryResult = [
      { threadId: 'thread-2', title: 'First prompt', updatedAt: '' },
      { threadId: 'thread-1', title: '', updatedAt: '' },
    ]
    desktopClientListener({ payload: chatEventsStatus(true) })

    await waitFor(() => expect(document.querySelector('[data-thread-id="thread-2"]')).toHaveTextContent('First prompt'))
    expect(document.querySelector('.thread-row[aria-current="true"]')).toHaveTextContent('New thread')
    expect(invoke).toHaveBeenCalledWith('chat_thread_open', { threadId: 'thread-1', limit: 100 })
    expect(invoke).toHaveBeenCalledWith('chat_current_thread')
    expect(invoke).not.toHaveBeenCalledWith('chat_select_thread', expect.anything())
  })

  it('keeps the owner notice when the desktop client supervisor stops', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    render(App)
    await findWorkspaceComposer()

    desktopClientListener({ payload: { connected: false, supervisor_running: true } })
    await vi.advanceTimersByTimeAsync(2_000)
    runtimeListener({ payload: { revision: 1, lastEvent: 'disconnected', visible: true } })
    expect(await screen.findByText('The runtime connection closed.')).toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: 'Message' })).not.toBeInTheDocument()

    desktopClientListener({ payload: { connected: false, supervisor_running: false } })
    expect(screen.getByText('The runtime connection closed.')).toBeInTheDocument()
    runtimeListener({ payload: { revision: 2, lastEvent: 'connected', visible: false } })
    expect(await screen.findByRole('textbox', { name: 'Message' })).toBeInTheDocument()
    expect(screen.queryByText('The runtime connection closed.')).not.toBeInTheDocument()
  })

  it('reads status after listener registration completes', async () => {
    let finishRegistration
    let connected = true
    desktopClientListen = vi.fn(() => new Promise((resolve) => { finishRegistration = resolve }))
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'attach_listener_status') return { connected, supervisor_running: true }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'chat_thread_summaries') return { summaries: [], nextCursor: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await waitFor(() => expect(desktopClientListen).toHaveBeenCalled())
    expect(invoke).not.toHaveBeenCalledWith('attach_listener_status')

    connected = false
    finishRegistration(desktopClientUnlisten)
    runtimeListener({ payload: { revision: 1, lastEvent: 'disconnected', visible: true } })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_listener_status'))
    expect(await screen.findByText('The runtime connection closed.')).toBeInTheDocument()
  })

  it('hides the workspace until the initial desktop client status arrives', async () => {
    let finishStatus
    invoke.mockImplementation((command) => {
      if (command === 'auth_status') return Promise.resolve({ signed_in: true, subject: 'token-subject' })
      if (command === 'attach_listener_status') return new Promise((resolve) => { finishStatus = resolve })
      if (command === 'chat_thread_open') return Promise.resolve([])
      if (command === 'auth_entitlement_snapshot') return Promise.resolve(snapshot())
      if (command === 'chat_thread_summaries') return Promise.resolve({ summaries: [], nextCursor: null })
      return Promise.reject(new Error(`unexpected command: ${command}`))
    })
    render(App)
    await waitFor(() => expect(finishStatus).toBeDefined())
    expect(screen.queryByRole('textbox', { name: 'Message' })).not.toBeInTheDocument()

    finishStatus({ connected: false, supervisor_running: true })
    runtimeListener({ payload: { revision: 1, lastEvent: 'disconnected', visible: true } })
    expect(await screen.findByText('The runtime connection closed.')).toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: 'Message' })).not.toBeInTheDocument()
  })

  it('keeps an event received during the initial status read', async () => {
    let finishStatus
    invoke.mockImplementation((command) => {
      if (command === 'auth_status') return Promise.resolve({ signed_in: true, subject: 'token-subject' })
      if (command === 'attach_listener_status') return new Promise((resolve) => { finishStatus = resolve })
      if (command === 'chat_thread_open') return Promise.resolve([])
      if (command === 'auth_entitlement_snapshot') return Promise.resolve(snapshot())
      if (command === 'chat_thread_summaries') return Promise.resolve({ summaries: [], nextCursor: null })
      return Promise.reject(new Error(`unexpected command: ${command}`))
    })
    render(App)
    await waitFor(() => expect(finishStatus).toBeDefined())

    desktopClientListener({ payload: { connected: true, supervisor_running: true } })
    finishStatus({ connected: false, supervisor_running: true })
    expect(await screen.findByRole('textbox', { name: 'Message' })).toBeInTheDocument()
    expect(screen.queryByText('Muniment cannot reach its background service.')).not.toBeInTheDocument()
  })

  it('keeps the workspace visible when the desktop owns its listener', async () => {
    render(App)

    expect(await screen.findByRole('textbox', { name: 'Message' })).toBeInTheDocument()
    expect(screen.queryByText('Muniment cannot reach its background service.')).not.toBeInTheDocument()
  })

  const upgradeStatus = (pending) => ({
    connected: true, chat_events_connected: true, supervisor_running: true, runtime_upgrade_pending: pending,
  })

  function mockRuntimeUpgrade({ pending, thread = [] } = {}) {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'attach_listener_status') return upgradeStatus(pending)
      if (command === 'chat_thread_open') return thread
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'chat_submit') return { runId: 'run-upgrade', attachments: [] }
      if (command === 'chat_resume') return { runId: 'run-interrupted' }
      if (command === 'chat_queue') return undefined
      throw new Error(`unexpected command: ${command}`)
    })
  }

  it('holds Send behind the update notice while the runtime upgrade is pending', async () => {
    mockRuntimeUpgrade({ pending: true })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })

    const notice = screen.getByText('A Muniment update is finishing.')
    expect(notice).toHaveClass('record', 'error-record')
    expect(notice.closest('section')).toHaveAttribute('aria-live', 'polite')
    expect(screen.getByText('Muniment resumes on its own.')).toHaveClass('support')

    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).toHaveAttribute('aria-disabled', 'true')
    expect(send).not.toBeDisabled()
    await fireEvent.click(send)
    await fireEvent.keyDown(composer, { key: 'Enter' })
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
  })

  it('drops the update notice and releases Send once the runtime upgrade finishes', async () => {
    mockRuntimeUpgrade({ pending: true })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).toHaveAttribute('aria-disabled', 'true')

    desktopClientListener({ payload: upgradeStatus(false) })

    await waitFor(() => expect(send).not.toHaveAttribute('aria-disabled'))
    expect(screen.queryByText('A Muniment update is finishing.')).not.toBeInTheDocument()
    expect(screen.queryByText('Muniment resumes on its own.')).not.toBeInTheDocument()
    await fireEvent.click(send)
    expect(invoke).toHaveBeenCalledWith('chat_submit', { prompt: 'A question', files: [] })
  })

  it('holds Resume while the runtime upgrade is pending and releases it when the flag clears', async () => {
    mockRuntimeUpgrade({ pending: true, thread: [{
      runId: 'run-interrupted', phase: 'interrupted', text: 'Partial answer',
      prompt: 'Original prompt', receipt: null, toolActivity: [], resumable: true,
    }] })
    render(App)
    const resume = await screen.findByRole('button', { name: 'Resume' })
    expect(resume).toBeDisabled()
    await fireEvent.click(resume)
    expect(invoke).not.toHaveBeenCalledWith('chat_resume', expect.anything())

    desktopClientListener({ payload: upgradeStatus(false) })

    await waitFor(() => expect(resume).not.toBeDisabled())
    await fireEvent.click(resume)
    expect(invoke).toHaveBeenCalledWith('chat_resume', { runId: 'run-interrupted' })
  })

  it('keeps the stop control and steers by Enter while a run is live', async () => {
    mockRuntimeUpgrade({ pending: false })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'Initial prompt' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    const stop = await screen.findByRole('button', { name: 'Stop' })
    expect(screen.queryByRole('button', { name: 'Send' })).not.toBeInTheDocument()
    expect(screen.queryByText(/follow-up|steers/)).not.toBeInTheDocument()
    await fireEvent.input(composer, { target: { value: 'Then summarize it' } })
    expect(stop).not.toHaveAttribute('aria-disabled')
    expect(screen.getAllByRole('button').filter((button) => button.closest('.composer-actions')).map((button) => button.getAttribute('aria-label') ?? button.textContent)).toEqual(['Voice', 'Stop'])

    await fireEvent.keyDown(screen.getByPlaceholderText('Ask anything'), { key: 'Enter' })
    expect(invoke).toHaveBeenCalledWith('chat_queue', { runId: 'run-upgrade', delivery: 'steer', message: 'Then summarize it' })
  })

  it('refuses Send during the first thread restore and keeps the draft for a retry', async () => {
    const restore = deferred()
    const fallback = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => {
      if (command === 'chat_thread_open') return restore.promise
      if (command === 'chat_submit') return { runId: 'run-after-restore', attachments: [] }
      return fallback(command, payload)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await waitFor(() => expect(composer).toBeDisabled())

    // A stale input event can land as the thread restore starts.
    await fireEvent.input(composer, { target: { value: 'Keep this prompt' } })
    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).toBeDisabled()
    expect(send).toHaveAttribute('aria-disabled', 'true')
    const status = screen.getByText('Send waits for the thread. Your draft stays here.')
    expect(status).toHaveAttribute('role', 'status')
    expect(composer).toHaveAccessibleDescription(status.textContent)
    await fireEvent.click(send)
    await fireEvent.keyDown(composer, { key: 'Enter' })
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(composer).toHaveValue('Keep this prompt')

    restore.resolve([])
    await waitFor(() => expect(send).toBeEnabled())
    expect(composer).toBeEnabled()
    expect(status).not.toBeInTheDocument()
    expect(composer).toHaveValue('Keep this prompt')
    await fireEvent.click(send)
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_submit')).toEqual([
      ['chat_submit', { prompt: 'Keep this prompt', files: [] }],
    ])
    expect(await screen.findByText('Keep this prompt', { selector: '.user-turn p' })).toBeInTheDocument()
    expect(composer).toHaveValue('')
  })

  it.each(['success', 'failure', 'rollback failure'])('refuses Send during a thread switch and releases it after %s', async (outcome) => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Current thread', updatedAt: '' },
      { threadId: 'thread-2', title: 'Other thread', updatedAt: '' },
    ]
    let open = deferred()
    const fallback = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => {
      if (command === 'chat_select_thread') {
        if (outcome === 'rollback failure' && payload.threadId === 'thread-1') return Promise.reject(new Error('Thread selection failed.'))
        return undefined
      }
      if (command === 'chat_thread_open' && payload.threadId === 'thread-2') return open.promise
      if (command === 'chat_submit') return { runId: 'run-after-switch', attachments: [] }
      return fallback(command, payload)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await waitFor(() => expect(composer).toBeEnabled())
    await fireEvent.input(composer, { target: { value: 'Keep this draft' } })
    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).toBeEnabled()
    await fireEvent.click(screen.getByRole('button', { name: /^Other thread/ }))

    expect(send).toBeDisabled()
    expect(composer).toBeDisabled()
    await fireEvent.click(send)
    await fireEvent.keyDown(composer, { key: 'Enter' })
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(composer).toHaveValue('Keep this draft')

    if (outcome === 'success') open.resolve([])
    else open.reject(new Error('Thread read failed.'))
    if (outcome !== 'success') {
      const retry = await screen.findByRole('button', { name: 'Restore history' })
      expect(invoke).toHaveBeenCalledWith('chat_select_thread', { threadId: 'thread-1' })
      if (outcome === 'rollback failure') {
        expect(send).toBeDisabled()
        expect(composer).toBeDisabled()
        expect(screen.getByText('Send waits for the thread. Your draft stays here.')).toBeVisible()
        await fireEvent.click(send)
        expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
        open = deferred()
        await fireEvent.click(retry)
        open.resolve([])
      }
    }
    await waitFor(() => expect(send).toBeEnabled())
    expect(composer).toHaveValue('Keep this draft')
    await fireEvent.click(send)
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_submit')).toEqual([
      ['chat_submit', { prompt: 'Keep this draft', files: [] }],
    ])
    expect(await screen.findByText('Keep this draft', { selector: '.user-turn p' })).toBeInTheDocument()
  })

  it('names and describes the composer in its default state', async () => {
    render(App)

    const composer = await findWorkspaceComposer()
    await waitFor(() => expect(composer).toBeEnabled())
    expect(composer).toHaveAccessibleDescription('Routing is automatic. Every reply carries its receipt.')
    expect(composer).toHaveAttribute('placeholder', 'Ask anything')
  })

  it('renders no mark or wordmark in the signed-in workspace', async () => {
    const { container } = render(App)

    await findWorkspaceComposer()

    expect(container.querySelector('.lockup')).not.toBeInTheDocument()
    expect(screen.queryByText(/shell v/)).not.toBeInTheDocument()
    expect(container.querySelector('.side-brand')).not.toBeInTheDocument()
    expect(screen.queryByText('muniment')).not.toBeInTheDocument()
  })

  it('focuses the composer once when the workspace appears and shows the send control with a draft', async () => {
    render(App)
    const composer = await findWorkspaceComposer()
    await waitFor(() => expect(composer).toBeEnabled())

    expect(composer).toHaveFocus()
    // The band's action control is absent while the draft is empty.
    expect(screen.queryByRole('button', { name: 'Send' })).not.toBeInTheDocument()
    await fireEvent.input(composer, { target: { value: 'A question' } })
    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).toHaveClass('primary', 'composer-action')
    expect(send).not.toHaveAttribute('title')
    expect(send.querySelector('[data-icon="arrow-up"]')).toBeInTheDocument()
    expect(send).not.toHaveAttribute('aria-disabled')
    expect(send).not.toBeDisabled()
    const disabledSendRule = appRules.get('.composer-actions .primary[aria-disabled="true"]')
    expect(disabledSendRule).toMatch(/background:\s*var\(--faint\)/)
    expect(disabledSendRule).toMatch(/border-color:\s*var\(--border\)/)
    expect(disabledSendRule).toMatch(/color:\s*var\(--muted\)/)

    const railToggle = screen.getByRole('button', { name: 'Open artifact rail' })
    railToggle.focus()
    await fireEvent.click(railToggle)
    expect(railToggle).toHaveFocus()
  })

  it('launches into local mode when no cloud session exists', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter') return undefined
      if (command === 'chat_thread_open') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('local_mode_enter')
    expect(invoke).not.toHaveBeenCalledWith('auth_sign_in')
    expect(screen.queryByRole('button', { name: 'Sign in' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Use local mode' })).not.toBeInTheDocument()
    expect(document.querySelector('.lockup')).not.toBeInTheDocument()
  })

  it('opens Settings as a popup with Models, Preferences, Home, Companies and Account sections', async () => {
    localModeStatus = true
    render(App)
    await screen.findByTestId('local-mode')
    expect(screen.queryByRole('button', { name: 'Sign in for cloud features' })).not.toBeInTheDocument()

    const settings = screen.getByRole('button', { name: 'Settings' })
    expect(settings).toHaveAttribute('aria-expanded', 'false')
    await fireEvent.click(settings)
    const dialog = await screen.findByRole('dialog', { name: 'Settings' })
    expect(settings).toHaveAttribute('aria-expanded', 'true')
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(screen.getByTestId('settings-scrim')).toContainElement(dialog)
    // The workspace under the popup blurs behind the theme's paper: dark in dark mode, light in light mode.
    expect(settingsStyles).toMatch(/\.settings-scrim \{[^}]*backdrop-filter:\s*blur\(/)
    expect(settingsStyles).toMatch(/\.settings-scrim \{[^}]*color-mix\(in srgb, var\(--paper\)/)
    const nav = within(dialog).getByRole('navigation', { name: 'Settings sections' })
    expect(within(nav).getAllByRole('button').map((button) => button.textContent)).toEqual(['Models & routing', 'Preferences', 'Profile & Memory', 'Home', 'Companies', 'Account'])
    expect(within(nav).getByRole('button', { name: 'Models & routing' })).toHaveAttribute('aria-current', 'true')
    expect(within(dialog).getByRole('button', { name: 'Connect account' })).toBeInTheDocument()
    await fireEvent.click(within(nav).getByRole('button', { name: 'Preferences' }))
    expect(within(dialog).getByRole('group', { name: 'Mode' })).toBeInTheDocument()
    await fireEvent.click(within(nav).getByRole('button', { name: 'Home' }))
    expect(within(dialog).getByRole('button', { name: 'Change folder…' })).toBeInTheDocument()
    await fireEvent.click(within(nav).getByRole('button', { name: 'Account' }))
    expect(within(dialog).getByRole('button', { name: 'Sign in for cloud features' })).toBeInTheDocument()

    await fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Settings' })).not.toBeInTheDocument())
    await waitFor(() => expect(document.activeElement).toBe(settings))
  })

  it('opens Settings from the settings shortcut with a collapsed sidebar and toggles it', async () => {
    localModeStatus = true
    localStorage.setItem('muniment.sidebar-collapsed', 'collapsed')
    render(App)
    expect(await screen.findByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Settings' })).not.toBeInTheDocument()
    const shortcut = navigator.platform.startsWith('Mac') ? { key: ',', metaKey: true } : { key: ',', ctrlKey: true }

    await fireEvent.keyDown(document, shortcut)
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument()
    // The popup needs no sidebar.
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()

    await fireEvent.keyDown(document, shortcut)
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Settings' })).not.toBeInTheDocument())
    await fireEvent.keyDown(document, shortcut)
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Close settings' }))
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Settings' })).not.toBeInTheDocument())

    await fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }))
    const settings = await screen.findByRole('button', { name: 'Settings' })
    expect(settings).toHaveAttribute('aria-keyshortcuts', navigator.platform.startsWith('Mac') ? 'Meta+,' : 'Control+,')
    expect(settings).not.toHaveAttribute('title')
  })


  it.each(['System', 'Light', 'Dark'])('persists %s from the local appearance control across remounts', async (label) => {
    localModeStatus = true
    localStorage.setItem('muniment.theme', 'light')
    render(App)
    await openSettings('Preferences')
    const appearance = within(await screen.findByRole('group', { name: 'Mode' }))
    expect(appearance.getAllByRole('button').map((button) => button.textContent)).toEqual(['System', 'Light', 'Dark'])
    expect(appearance.getByRole('button', { name: 'Light' })).toHaveAttribute('aria-pressed', 'true')
    const themes = within(screen.getByRole('group', { name: 'Themes' }))
    expect(themes.getAllByRole('button').map((button) => button.textContent.trim()).slice(0, 4)).toEqual(['Paper', 'Vellum', 'Ledger', 'Foolscap'])

    await fireEvent.click(appearance.getByRole('button', { name: label }))
    expect(JSON.parse(localStorage.getItem('muniment.theme'))).toEqual({ mode: label.toLowerCase(), light: 'paper', dark: 'vault' })
    expect(document.documentElement.dataset.theme).toBe({ System: undefined, Light: 'paper', Dark: 'vault' }[label])
    expect(appearance.getByRole('button', { name: label })).toHaveAttribute('aria-pressed', 'true')

    cleanup()
    render(App)
    // The sidebar carries no appearance control, so the workspace stays clear of it.
    const reopened = await screen.findByRole('button', { name: 'Settings' })
    expect(screen.queryByRole('group', { name: 'Mode' })).not.toBeInTheDocument()
    await fireEvent.click(reopened)
    await fireEvent.click(within(await screen.findByRole('dialog', { name: 'Settings' })).getByRole('button', { name: 'Preferences' }))
    const restored = within(await screen.findByRole('group', { name: 'Mode' }))
    expect(restored.getByRole('button', { name: label })).toHaveAttribute('aria-pressed', 'true')
    expect(invoke).not.toHaveBeenCalledWith('auth_sign_in')
    delete document.documentElement.dataset.theme
  })

  it('steps the body type size from Preferences and the super key, and picks installed fonts per register', async () => {
    localModeStatus = true
    localStorage.removeItem('muniment.type')
    render(App)
    await openSettings('Preferences')
    const size = within(await screen.findByRole('group', { name: 'Size' }))
    expect(size.getByRole('status')).toHaveTextContent('15 px body')
    expect(size.getByRole('button', { name: 'Default type size' })).toBeDisabled()
    const mac = navigator.platform.startsWith('Mac')
    expect(size.getByRole('button', { name: 'Larger type' })).toHaveAttribute('aria-keyshortcuts', mac ? 'Meta+=' : 'Control+=')

    await fireEvent.click(size.getByRole('button', { name: 'Larger type' }))
    expect(size.getByRole('status')).toHaveTextContent('16.5 px body')
    expect(document.documentElement.style.getPropertyValue('--text-15')).toBe('16.5px')
    expect(document.documentElement.style.getPropertyValue('--text-provenance')).toBe('12.7px')
    expect(JSON.parse(localStorage.getItem('muniment.type'))).toEqual({ step: 1, human: null, mono: null })

    await fireEvent.keyDown(document, { key: '=', metaKey: mac, ctrlKey: !mac })
    expect(size.getByRole('status')).toHaveTextContent('18 px body')
    await fireEvent.keyDown(document, { key: '-', metaKey: mac, ctrlKey: !mac })
    expect(size.getByRole('status')).toHaveTextContent('16.5 px body')
    await fireEvent.keyDown(document, { key: '0', metaKey: mac, ctrlKey: !mac })
    expect(size.getByRole('status')).toHaveTextContent('15 px body')
    expect(document.documentElement.style.getPropertyValue('--text-15')).toBe('')
    expect(size.getByRole('button', { name: 'Default type size' })).toBeDisabled()

    const human = within(await screen.findByRole('group', { name: 'Human font' }))
    expect(human.getByRole('button', { name: /Schibsted Grotesk/ })).toHaveAttribute('aria-pressed', 'true')
    await waitFor(() => expect(human.getByRole('button', { name: 'Inter' })).toBeInTheDocument())
    await fireEvent.input(human.getByRole('searchbox', { name: 'Search human fonts' }), { target: { value: 'ios' } })
    expect(human.getAllByRole('button').map((button) => button.querySelector('.font-name').textContent)).toEqual(['Schibsted Grotesk', 'Iosevka'])
    await fireEvent.click(human.getByRole('button', { name: 'Iosevka' }))
    expect(document.documentElement.style.getPropertyValue('--font-human')).toBe("'Iosevka', 'Schibsted Grotesk', system-ui, sans-serif")
    expect(JSON.parse(localStorage.getItem('muniment.type'))).toEqual({ step: 0, human: 'Iosevka', mono: null })
    const mono = within(screen.getByRole('group', { name: 'Mono font' }))
    expect(mono.getByRole('button', { name: /Commit Mono/ })).toHaveAttribute('aria-pressed', 'true')
    expect(document.documentElement.style.getPropertyValue('--font-mono')).toBe('')

    await fireEvent.click(human.getByRole('button', { name: /Schibsted Grotesk/ }))
    expect(document.documentElement.style.getPropertyValue('--font-human')).toBe('')
    expect(JSON.parse(localStorage.getItem('muniment.type'))).toEqual({ step: 0, human: null, mono: null })
  })

  it('offers each provider its methods and the form for the chosen one', async () => {
    localModeStatus = true
    render(App)
    const dialog = await openSettings()
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Connect account' }))
    expect(within(dialog).getByRole('heading', { name: 'Popular' })).toBeInTheDocument()
    expect(within(dialog).getByRole('heading', { name: 'Other' })).toBeInTheDocument()
    // The featured eight lead, in the SPEC order.
    const popular = within(dialog).getByRole('heading', { name: 'Popular' }).nextElementSibling
    expect([...popular.querySelectorAll('button')].map((button) => button.querySelector('span:not(.logo)').textContent)).toEqual(['Anthropic', 'OpenAI', 'xAI', 'Google', 'OpenRouter', 'Ollama', 'LM Studio', 'Custom OpenAI-compatible endpoint'])

    await fireEvent.click(within(dialog).getByRole('button', { name: /^Ollama/ }))
    expect(within(dialog).getByLabelText('Ollama server URL')).toHaveValue('http://localhost:11434/v1')
    expect(within(dialog).getByRole('button', { name: 'Save Ollama server' })).toBeEnabled()

    await fireEvent.click(within(dialog).getByRole('button', { name: 'Back' }))
    await fireEvent.click(within(dialog).getByRole('button', { name: /^OpenAI/ }))
    // The account leads on one view. The key is one switch away, never a second list.
    expect(within(dialog).getByRole('button', { name: 'Sign in' })).toBeInTheDocument()
    expect(within(dialog).getByText(/Sign in with your ChatGPT Plus or Pro account\./)).toBeInTheDocument()
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Use an API key instead' }))
    expect(within(dialog).getByLabelText('OpenAI API key')).toBeVisible()
    expect(within(dialog).getByRole('button', { name: 'Save key' })).toBeDisabled()
    expect(within(dialog).queryByLabelText('Ollama server URL')).not.toBeInTheDocument()
    await fireEvent.click(within(dialog).getByRole('button', { name: /Sign in with your ChatGPT Plus or Pro account instead/ }))
    expect(within(dialog).getByRole('button', { name: 'Sign in' })).toBeInTheDocument()

    await fireEvent.click(within(dialog).getByRole('button', { name: 'Back' }))
    await fireEvent.click(within(dialog).getByRole('button', { name: /^xAI/ }))
    expect(within(dialog).getByText(/Sign in with your SuperGrok or X Premium account\./)).toBeInTheDocument()
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Back' }))
    await fireEvent.click(within(dialog).getByRole('button', { name: /^Anthropic/ }))
    expect(within(dialog).getByText(/Anthropic through your Claude Code sign-in\./)).toBeInTheDocument()
    expect(within(dialog).getByRole('button', { name: 'Use an API key instead' })).toBeInTheDocument()
  })

  it('opens provider sign-in from the account connector', async () => {
    localModeStatus = true
    const defaultInvoke = invoke.getMockImplementation()
    invoke.mockImplementation((command, ...args) => {
      if (command === 'local_mode_provider_inventory') return Promise.resolve({
        providers: [
          { id: 'google', name: 'Google', source: 'key', base_url: null, models: [] },
          { id: 'ollama', name: 'Ollama', source: 'local', base_url: 'http://localhost:11434/v1', models: [{ id: 'llama3.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false }] },
        ],
        default_provider: 'ollama',
        default_model: 'llama3.2:3b',
        hidden: [],
      })
      return defaultInvoke(command, ...args)
    })
    render(App)
    const dialog = await openSettings()
    await within(dialog).findByRole('region', { name: 'Ollama' })
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Connect account' }))
    const connect = dialog
    await fireEvent.click(within(connect).getByRole('button', { name: /^OpenAI/ }))
    expect(within(dialog).getByRole('button', { name: 'Sign in' })).toBeInTheDocument()
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Back' }))
    expect(within(dialog).getByRole('button', { name: /^OpenAI/ })).toBeInTheDocument()
  })

  it('draws the models the shell holds before the fresh inventory read answers', async () => {
    localModeStatus = true
    const ollama = { id: 'ollama', name: 'Ollama', source: 'local', base_url: 'http://localhost:11434/v1', models: [{ id: 'llama3.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false }] }
    const held = { providers: [ollama], default_provider: 'ollama', default_model: 'llama3.2:3b', hidden: [] }
    const fresh = deferred()
    let reads = 0
    const defaultInvoke = invoke.getMockImplementation()
    invoke.mockImplementation((command, ...args) => {
      if (command === 'local_mode_provider_inventory') return ++reads === 1 ? Promise.resolve(held) : fresh.promise
      return defaultInvoke(command, ...args)
    })
    render(App)
    await waitFor(() => expect(reads).toBe(1))
    const dialog = await openSettings()
    // The startup read seeds the page, so the list shows while the fresh read runs.
    expect(within(dialog).getByRole('region', { name: 'Ollama' })).toBeInTheDocument()
    expect(within(dialog).getByText('llama3.2:3b')).toBeInTheDocument()
    expect(reads).toBe(2)
    fresh.resolve({ ...held, providers: [{ ...ollama, models: [...ollama.models, { id: 'granite4.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false }] }] })
    expect(await within(dialog).findByText('granite4.2:3b')).toBeInTheDocument()
  })

  it.each([false, true])('disables the endpoint form until a save settles with failure %s', async (fails) => {
    localModeStatus = true
    const save = deferred()
    const defaultInvoke = invoke.getMockImplementation()
    invoke.mockImplementation((command, ...args) => {
      if (command === 'local_mode_store_local_provider') return save.promise
      if (command === 'local_mode_provider_inventory') return Promise.resolve(emptyInventory)
      return defaultInvoke(command, ...args)
    })
    render(App)
    const dialog = await openSettings()
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Connect account' }))
    await fireEvent.click(within(dialog).getByRole('button', { name: /^Ollama/ }))
    const url = within(dialog).getByLabelText('Ollama server URL')
    await fireEvent.input(url, { target: { value: 'http://localhost:11434/v1' } })
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Save Ollama server' }))
    expect(url).toBeDisabled()
    expect(within(dialog).getByRole('button', { name: 'Save Ollama server' })).toBeDisabled()

    if (fails) save.reject(new Error('Ollama setup failed. Check the URL, then retry.'))
    else save.resolve()
    if (fails) {
      expect(await within(dialog).findByText('Ollama setup failed. Check the URL, then retry.')).toBeInTheDocument()
      await waitFor(() => expect(url).toBeEnabled())
    } else {
      expect(await within(dialog).findByText('Muniment saved the Ollama server.')).toBeInTheDocument()
      expect(within(dialog).getByRole('button', { name: 'Connect account' })).toBeInTheDocument()
    }
  })


  it('shows a delivery deadline cause and restores the local reply from the journal', async () => {
    localModeStatus = true
    let restored = false
    const defaultInvoke = invoke.getMockImplementation()
    invoke.mockImplementation((command, ...args) => {
      if (command === 'chat_current_thread') return 'thread-1'
      if (command === 'chat_thread_open') return [{
        runId: 'run-local', phase: restored ? 'complete' : 'streaming',
        text: restored ? 'The journal kept the reply.' : 'A', prompt: 'A question', receipt: {}, toolActivity: [],
      }]
      return defaultInvoke(command, ...args)
    })
    render(App)
    await screen.findByText('A question', { selector: '.user-turn p' })
    restored = true
    const cause = 'Reply delivery failed. The desktop missed the five-second chat.event frame deadline with 17 bytes pending.'
    chatListener({ payload: { runId: 'run-local', phase: 'delivery-failed', failureReason: cause } })
    expect(await screen.findByText(cause)).toBeVisible()
    expect(await screen.findByText('The journal kept the reply.')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Restore reply' })).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Stop' })).not.toBeInTheDocument()
  })


  it('blocks sign-in while local mode entry is pending', async () => {
    const localEntry = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter') return localEntry.promise
      if (command === 'chat_thread_open') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    const signIn = await screen.findByRole('button', { name: 'Sign in' })
    await fireEvent.click(screen.getByRole('button', { name: 'Use local mode' }))

    expect(signIn).toBeDisabled()
    await fireEvent.click(signIn)
    expect(invoke).not.toHaveBeenCalledWith('auth_sign_in')

    localEntry.resolve()
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
  })

  it('waits for the local marker before a connection refresh checks auth', async () => {
    const marker = deferred()
    localModeStatus = marker.promise
    invoke.mockImplementation(async (command) => {
      if (command === 'attach_listener_status') return { connected: true, supervisor_running: true }
      if (command === 'chat_thread_open') return []
      throw new Error(`unexpected command: ${command}`)
    })

    render(App)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_listener_status'))
    expect(invoke).not.toHaveBeenCalledWith('auth_status')

    marker.resolve(true)

    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('auth_status')
  })

  it('skips the connection auth check after launching into local mode', async () => {
    const refresh = deferred()
    let authChecks = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') {
        authChecks += 1
        if (authChecks === 1) return { signed_in: false, subject: null }
        return refresh.promise
      }
      if (command === 'attach_listener_status') return { connected: false, supervisor_running: true }
      if (command === 'local_mode_enter') return undefined
      if (command === 'chat_thread_open') return []
      throw new Error(`unexpected command: ${command}`)
    })

    render(App)
    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    desktopClientListener({ payload: { connected: true, supervisor_running: true } })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('attach_listener_status'))

    expect(authChecks).toBe(1)
    refresh.resolve({ signed_in: false, subject: null })
    await waitFor(() => expect(screen.getByPlaceholderText('Ask anything')).toBeInTheDocument())
    expect(screen.queryByRole('button', { name: 'Sign in' })).not.toBeInTheDocument()
  })

  it('uses the native marker when web storage disagrees', async () => {
    localStorage.setItem('muniment.local-mode', 'false')
    localModeStatus = true
    invoke.mockImplementation(async (command) => {
      if (command === 'chat_thread_open') return []
      throw new Error(`unexpected command: ${command}`)
    })

    render(App)

    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke.mock.calls.filter(([command]) => command.startsWith('auth_'))).toHaveLength(0)

    cleanup()
    localModeStatus = false
    localStorage.setItem('muniment.local-mode', 'true')
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter') return undefined
      if (command === 'chat_thread_open') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    // The marker wins over web storage: the shell asks for the session, then enters local mode.
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('auth_status'))
    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('local_mode_enter')
  })

  it('keeps one focused sign-in button while browser sign-in is pending', async () => {
    const signInRequest = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'auth_sign_in') return signInRequest.promise
      if (command === 'local_mode_enter') throw new Error('marker refused')
      throw new Error(`unexpected command: ${command}`)
    })
    const { container } = render(App)
    const signIn = await screen.findByRole('button', { name: 'Sign in' })
    // The sign-in screen stays only after local mode refused to start.
    await screen.findByRole('alert')
    expect(screen.getByText('Sign in for cloud features, or use local mode.')).toHaveAttribute('aria-live', 'polite')
    expect(container.querySelectorAll('[aria-live="polite"]')).toHaveLength(1)
    expect(signIn).not.toHaveAttribute('aria-live')
    signIn.focus()

    await fireEvent.click(signIn)

    expect(screen.getByText('Waiting for the browser sign-in…')).toHaveAttribute('aria-live', 'polite')
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1)
    expect(screen.getByRole('heading', { level: 1, name: 'muniment' })).toBeInTheDocument()
    expect(container.querySelectorAll('[aria-live="polite"]')).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'Sign in' })).toBe(signIn)
    expect(signIn).toHaveFocus()
    expect(signIn).toHaveAttribute('aria-disabled', 'true')
    expect(signIn).not.toBeDisabled()
    expect(signIn).toHaveClass('inactive')
    expect(container.querySelectorAll('button')).toHaveLength(2)
    expect(appRules.get('button[aria-disabled="true"].inactive')).toMatch(/color:\s*var\(--muted\)/)
    expect(appRules.get('button[aria-disabled="true"].inactive')).toMatch(/cursor:\s*default/)
    expect(appRules.has('button:hover:not(:disabled):not([aria-disabled="true"])')).toBe(true)

    await fireEvent.click(signIn)
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_sign_in')).toHaveLength(1)
  })

  it.each(['button', 'Escape'])('cancels browser sign-in through %s and ignores a late success', async (control) => {
    const signInRequest = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter' || command === 'local_mode_leave') return undefined
      if (command === 'auth_sign_in') return signInRequest.promise
      if (command === 'chat_thread_open') return []
      if (command === 'local_mode_provider_inventory') return { providers: [], hidden: [] }
      throw new Error(`unexpected command: ${command}`)
    })
    const { container } = render(App)
    await screen.findByTestId('local-mode')
    const composer = screen.getByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Keep my draft' } })
    await openSettings('Account')
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in for cloud features' }))
    const cancel = await screen.findByRole('button', { name: 'Cancel sign-in' })
    expect(container.querySelector('.workspace')).toHaveProperty('inert', true)
    expect(composer).toHaveValue('Keep my draft')
    if (control === 'button') await fireEvent.click(cancel)
    else await fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => expect(cancel).toBeDisabled())
    signInRequest.resolve({ signed_in: true, subject: 'late-user' })
    await screen.findByTestId('local-mode')
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Cancel sign-in' })).not.toBeInTheDocument())
    expect(container.querySelector('.workspace')).toHaveProperty('inert', false)
    expect(screen.getByPlaceholderText('Ask anything')).toHaveValue('Keep my draft')
  })

  it('shows the registration wait and completes without another user action', async () => {
    const signInRequest = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter' || command === 'local_mode_leave') return undefined
      if (command === 'auth_sign_in') return signInRequest.promise
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByTestId('local-mode')
    await openSettings('Account')
    const cloudSignIn = await screen.findByRole('button', { name: 'Sign in for cloud features' })
    await waitFor(() => expect(cloudSignIn).toBeEnabled())
    await fireEvent.click(cloudSignIn)
    await waitFor(() => expect(registrationRetryListener).toBeDefined())

    registrationRetryListener({ payload: { delay_seconds: 30 } })

    expect(await screen.findByText('Server busy. Retrying in 30 s')).toBeInTheDocument()
    expect(screen.queryByText(/Sign-in not completed/)).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument()

    signInRequest.resolve({ signed_in: true, subject: 'user-a' })
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_sign_in')).toHaveLength(1)
  })

  it('shows the sign-in link the runtime announces while the browser sign-in waits', async () => {
    const signInRequest = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter' || command === 'local_mode_leave') return undefined
      if (command === 'auth_sign_in') return signInRequest.promise
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByTestId('local-mode')
    await openSettings('Account')
    const cloudSignIn = await screen.findByRole('button', { name: 'Sign in for cloud features' })
    await waitFor(() => expect(cloudSignIn).toBeEnabled())
    await fireEvent.click(cloudSignIn)
    await waitFor(() => expect(chatListener).toBeDefined())
    expect(screen.queryByTestId('sign-in-link')).not.toBeInTheDocument()

    chatListener({ payload: { runId: 'sign-in', phase: 'sign-in-link', text: 'https://muniment.ai/authorize?state=abc' } })

    const link = await screen.findByTestId('sign-in-link')
    expect(link).toHaveAttribute('href', 'https://muniment.ai/authorize?state=abc')
    expect(link).toHaveTextContent('Open the sign-in page')
    expect(screen.getByText('Waiting for the browser sign-in…')).toBeInTheDocument()

    signInRequest.resolve({ signed_in: true, subject: 'user-a' })
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    await waitFor(() => expect(screen.queryByTestId('sign-in-link')).not.toBeInTheDocument())
  })

  it('shows the terminal screen for a non-retryable registration error', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'local_mode_enter' || command === 'local_mode_leave') return undefined
      if (command === 'auth_sign_in') throw new Error('native installation registration failed')
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByTestId('local-mode')
    await openSettings('Account')
    const cloudSignIn = await screen.findByRole('button', { name: 'Sign in for cloud features' })
    await waitFor(() => expect(cloudSignIn).toBeEnabled())
    await fireEvent.click(cloudSignIn)

    expect(await screen.findByText(/Sign-in not completed: Error: native installation registration failed/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument()
  })

  it('does not focus a composer in the auth-error state', async () => {
    invoke.mockRejectedValue('Authentication is unavailable.')
    render(App)

    expect(await screen.findByRole('button', { name: 'Try again' })).not.toHaveFocus()
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
  })

  it('focuses the first-run composer without a setup heading', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    render(App)

    expect(await screen.findByRole('textbox', { name: 'Message' })).toHaveFocus()
    expect(screen.getByRole('region', { name: 'First run' })).toBeInTheDocument()
    expect(document.querySelector('.lockup .name')?.tagName).toBe('SPAN')
    expect(screen.queryByRole('heading', { name: 'Choose your Muniment Home' })).not.toBeInTheDocument()
  })

  it('releases composer focus in Home settings and restores it on workspace re-entry', async () => {
    render(App)
    const composer = await findWorkspaceComposer()
    expect(composer).toHaveFocus()

    await openSettings('Home')
    await fireEvent.click(await screen.findByRole('button', { name: 'Change folder…' }))
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-cancel'))

    expect(await screen.findByPlaceholderText('Ask anything')).toHaveFocus()
  })

  it('waits to focus on workspace re-entry until a resuming composer becomes enabled', async () => {
    let resolveResume
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-interrupted', phase: 'interrupted', text: 'Partial answer',
        prompt: 'Original prompt', receipt: null, toolActivity: [], resumable: true,
      }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_resume') return new Promise((resolve) => { resolveResume = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Resume' }))
    await openSettings('Home')
    await fireEvent.click(await screen.findByRole('button', { name: 'Change folder…' }))
    await fireEvent.click(screen.getByTestId('onboarding-cancel'))

    const composer = await screen.findByRole('textbox', { name: 'Message' })
    expect(composer).toBeDisabled()
    expect(composer).toHaveAccessibleDescription('Reopening the existing secure session…')
    expect(composer).toHaveAttribute('placeholder', 'Resuming interrupted reply…')
    expect(composer).not.toHaveFocus()

    chatListener({ payload: {
      runId: 'run-interrupted', phase: 'complete', text: 'Finished answer',
      receipt: { id: 'receipt-1' }, toolActivity: [], pendingPermission: null,
    } })
    await waitFor(() => expect(composer).toBeEnabled())
    expect(composer).toHaveFocus()

    const railToggle = screen.getByRole('button', { name: 'Open artifact rail' })
    railToggle.focus()
    resolveResume({ runId: 'run-interrupted' })
    await waitFor(() => expect(screen.queryByText('Resuming…')).not.toBeInTheDocument())
    expect(railToggle).toHaveFocus()
  })


})

// A company with records opens on its report. The kind list is one step away, behind Kinds.
async function openKinds(panel) {
  await fireEvent.click(await within(panel).findByRole('button', { name: 'Kinds' }))
  return within(panel).findByRole('navigation', { name: 'Kinds' })
}

describe('record panel', () => {
  it('sits flush right of Artifacts with its shortcut, opens on the report with the kind list behind Kinds, and closes Artifacts', async () => {
    render(App)
    const record = await screen.findByRole('button', { name: 'Open record panel' })
    const artifacts = screen.getByRole('button', { name: 'Open artifact rail' })
    const mac = navigator.platform.startsWith('Mac')
    expect(record).toHaveAttribute('aria-controls', 'record-panel')
    expect(record).toHaveAttribute('aria-keyshortcuts', mac ? 'Meta+K' : 'Control+K')
    expect(within(record).getByText(mac ? '⌘ K' : 'Ctrl K').tagName).toBe('KBD')
    const row = record.closest('.titlebar-thread') ?? record.parentElement
    const controls = [...row.querySelectorAll('.row-control')]
    expect(controls.indexOf(artifacts)).toBeLessThan(controls.indexOf(record))
    expect(controls.at(-1)).toBe(record)

    await fireEvent.click(artifacts)
    expect(screen.getByRole('complementary', { name: 'Artifacts' })).toBeInTheDocument()
    await fireEvent.click(record)
    expect(record).toHaveAttribute('aria-expanded', 'true')
    expect(record).toHaveAccessibleName('Close record panel')
    expect(artifacts).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByRole('complementary', { name: 'Artifacts' })).not.toBeInTheDocument()
    const panel = screen.getByRole('complementary', { name: 'Record' })
    expect(within(panel).getByRole('heading', { level: 2, name: 'Record' })).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('record_companies')
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_kinds', { companyId: 'company-1' }))
    const picker = await within(panel).findByRole('combobox', { name: 'Company' })
    expect(picker).toHaveValue('company-1')
    expect(within(picker).getAllByRole('option').map((option) => option.textContent)).toEqual(['Northwind', 'Surfoff'])
    await within(panel).findByRole('region', { name: 'Report' })
    expect(invoke).toHaveBeenCalledWith('record_report', { companyId: 'company-1' })
    const kinds = await openKinds(panel)
    const names = within(kinds).getAllByRole('button').map((button) => button.querySelector('.record-kind-name').textContent)
    expect(names).toEqual(['person', 'deal', 'vendor'])
    expect(within(kinds).getAllByRole('button')[1]).toHaveTextContent('2 properties, 2 states')

    await fireEvent.change(picker, { target: { value: 'company-2' } })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_company_select', { companyId: 'company-2' }))

    await fireEvent.click(record)
    expect(screen.queryByRole('complementary', { name: 'Record' })).not.toBeInTheDocument()
  })

  it('opens a kind as a generated table, a row as the record view, and edits a cell through propose and commit', async () => {
    render(App)
    const record = await screen.findByRole('button', { name: 'Open record panel' })
    await fireEvent.click(record)
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[1])
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_query', expect.objectContaining({ kind: 'deal', sort: 'updated_at', descending: true })))
    const table = await within(panel).findByRole('table', { name: 'deal records' })
    const headers = within(table).getAllByRole('columnheader').map((header) => header.textContent.trim())
    expect(headers).toEqual(['title', 'state', 'updated v', 'name'])
    expect(within(table).getAllByRole('columnheader')[1]).toHaveClass('mono')
    expect(within(table).getAllByRole('columnheader')[0]).not.toHaveClass('mono')
    expect(within(panel).getByText('1 record')).toBeInTheDocument()
    const row = within(table).getAllByRole('row')[1]
    expect(within(row).getAllByRole('cell')[1]).toHaveTextContent('Won')
    expect(within(row).getAllByRole('cell')[1]).toHaveClass('mono')
    expect(within(row).getAllByRole('cell')[2]).toHaveTextContent('2026-09-15 10:30')

    await fireEvent.click(within(table).getByRole('columnheader', { name: /title/ }).querySelector('button'))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_query', expect.objectContaining({ sort: 'title', descending: false })))

    const nameCell = within(row).getAllByRole('cell')[3]
    await fireEvent.dblClick(nameCell)
    const input = within(panel).getByRole('textbox', { name: 'name for Northwind renewal' })
    expect(input).toHaveValue('Northwind renewal')
    await fireEvent.input(input, { target: { value: 'Northwind renewal FY27' } })
    await fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'update', entity: 'entity:deal-1', data: { name: 'Northwind renewal FY27' } } }))
    const change = await within(panel).findByRole('group', { name: 'Proposed change' })
    expect(change).toHaveTextContent('name: Northwind renewal to Northwind renewal FY27')
    expect(change).toHaveTextContent('Northwind Traders has an open deal closing 2026-09-16')
    await fireEvent.click(within(change).getByRole('button', { name: 'Commit' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_commit', { companyId: 'company-1', proposal: 'proposal-1' }))
    await waitFor(() => expect(within(panel).queryByRole('group', { name: 'Proposed change' })).not.toBeInTheDocument())

    await fireEvent.click(within(panel).getByRole('button', { name: 'Northwind renewal' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_entity', { companyId: 'company-1', entity: 'deal-1' }))
    const view = await within(panel).findByRole('article', { name: 'Northwind renewal' })
    expect(view).toHaveTextContent('Northwind renewal: won.')
    expect(within(view).getByRole('region', { name: 'Identities' })).toHaveTextContent('external: hubspot:deal:1')
    expect(within(view).getByRole('region', { name: 'Relations' })).toHaveTextContent('concerns')
    expect(within(view).getByRole('button', { name: 'Northwind' })).toBeInTheDocument()
    expect(within(view).getByRole('region', { name: 'History' })).toHaveTextContent('2026-09-15 10:00 · Created · Unknown actor')

    await fireEvent.click(within(panel).getByRole('button', { name: 'Back' }))
    await within(panel).findByRole('table', { name: 'deal records' })
    await fireEvent.click(within(panel).getByRole('button', { name: 'Back' }))
    expect(within(panel).getByRole('navigation', { name: 'Kinds' })).toBeInTheDocument()
  })

  it('proposes a new record from a form generated from the kind and commits it', async () => {
    recordProposeResult = { proposal: { id: 'proposal-2', warnings: [], diff: { op: 'create', after: { id: 'deal-2', data: { name: 'Contoso pilot', stage: 'discovery', expected_close: '2026-12-01' } } } } }
    recordCommitResult = { result: { event_seq: 5, entity_ids: ['deal-2'] } }
    recordKindsResult = { company_id: 'company-1', kinds: [{ name: 'deal', schema: { properties: { name: { type: 'string' }, stage: { type: 'string', enum: ['discovery', 'won'] }, expected_close: { type: 'string', format: 'date' }, amount: { type: 'number' } }, required: ['name', 'stage', 'expected_close'], stateProperty: 'stage' }, states: ['discovery', 'won'], extension: null }] }
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[0])
    await within(panel).findByRole('table', { name: 'deal records' })
    await fireEvent.click(within(panel).getByRole('button', { name: 'New deal' }))
    const form = within(panel).getByRole('form', { name: 'New deal' })
    const labels = [...form.querySelectorAll('.record-field-label')].map((label) => label.textContent)
    expect(labels).toEqual(['stage *', 'name *', 'expected close *', 'amount'])
    const propose = within(form).getByRole('button', { name: 'Propose' })
    expect(propose).toBeDisabled()
    await fireEvent.change(form.querySelector('select'), { target: { value: 'discovery' } })
    await fireEvent.input(form.querySelectorAll('input')[0], { target: { value: 'Contoso pilot' } })
    await fireEvent.input(form.querySelectorAll('input')[1], { target: { value: '2026-12-01' } })
    expect(propose).toBeEnabled()
    await fireEvent.click(propose)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'create', kind: 'deal', data: { stage: 'discovery', name: 'Contoso pilot', expected_close: '2026-12-01' } } }))
    const proposed = await within(form).findByRole('group', { name: 'Proposed record' })
    expect(proposed).toHaveTextContent('name: Contoso pilot')
    await fireEvent.click(within(proposed).getByRole('button', { name: 'Commit' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_commit', { companyId: 'company-1', proposal: 'proposal-2' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_entity', { companyId: 'company-1', entity: 'deal-2' }))
  })

  it('imports a CSV file onto the open kind through a proposed mapping, runs it and lists the rows it could not place', async () => {
    recordKindsResult = { company_id: 'company-1', kinds: [{ name: 'org', schema: { properties: { name: { type: 'string' }, domain: { type: 'string' }, industry: { type: 'string' } }, required: ['name'] }, states: null, extension: { properties: { x_since: { type: 'string', format: 'date' } } } }] }
    recordQueryResult = { page: { kind: 'org', total: 0, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [] } }
    recordProposeResult = { proposal: { id: 'proposal-5', warnings: [], diff: { op: 'create', after: { id: 'map-1', data: { source: 'csv', object: '/exports/customers.csv', kind: 'org', fields: { Company: 'name', Website: 'domain', Since: 'x_since' }, identity: 'domain:Website', approved: true } } } } }
    recordCommitResult = { result: { event_seq: 6, entity_ids: ['map-1'] } }
    dialogResult = '/exports/customers.csv'
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[0])
    await within(panel).findByText('No org records yet')
    await fireEvent.click(within(panel).getByRole('button', { name: 'Import' }))
    const sources = within(panel).getByRole('list', { name: 'Sources' })
    expect(within(sources).getAllByRole('button').map((button) => button.querySelector('.record-import-source-name').textContent)).toEqual(['CSV file', 'Stripe', 'HubSpot', 'Pipedrive', 'Salesforce', 'Zendesk', 'Intercom', 'Freshdesk', 'Zoho CRM', 'Outreach', 'Salesloft', 'Notion', 'Airtable', 'Google Sheets', 'Square', 'Shopify', 'PayPal', 'FreshBooks', 'QuickBooks', 'Wave', 'Apollo', 'Gong', 'ZoomInfo', 'Calendly', 'Mailchimp', 'Kit', 'Xero', 'Dynamics 365', 'Marketo'])
    expect(within(sources).getAllByRole('button')[0]).toHaveTextContent('a file on this machine')
    await fireEvent.click(within(sources).getByRole('button', { name: /CSV file/ }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_describe', { companyId: 'company-1', source: 'csv', object: '/exports/customers.csv' }))
    const form = await within(panel).findByRole('form', { name: 'Map customers.csv' })
    expect(form).toHaveTextContent('customers.csv · 3 rows')
    const columns = within(form).getByRole('table', { name: 'Columns' })
    expect(within(columns).getAllByRole('rowheader').map((cell) => cell.textContent)).toEqual(['Company', 'Website', 'Since'])
    expect(within(columns).getByRole('combobox', { name: 'Property for Company' })).toHaveValue('name')
    expect(within(columns).getByRole('combobox', { name: 'Property for Website' })).toHaveValue('domain')
    const since = within(columns).getByRole('combobox', { name: 'Property for Since' })
    expect(since).toHaveValue('x_since')
    expect(within(since).getByRole('option', { name: 'since (own)' })).toBeInTheDocument()
    expect(within(since).getByRole('option', { name: 'skip' })).toBeInTheDocument()
    const identity = within(form).getByRole('combobox', { name: 'Identity' })
    expect(identity).toHaveValue('domain:Website')
    expect(within(identity).getAllByRole('option').map((option) => option.textContent)).toEqual(['the title', 'domain in Website', 'id in Company', 'id in Website', 'id in Since'])

    await fireEvent.click(within(form).getByRole('button', { name: 'Propose mapping' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'create', kind: 'mapping', data: { source: 'csv', object: '/exports/customers.csv', kind: 'org', fields: { Company: 'name', Website: 'domain', Since: 'x_since' }, approved: true, identity: 'domain:Website' } } }))
    const proposed = await within(form).findByRole('group', { name: 'Proposed mapping' })
    expect(proposed).toHaveTextContent('Company fills name')
    expect(proposed).toHaveTextContent('keyed on domain in Website')
    expect(proposed).toHaveTextContent('kind org')
    await fireEvent.click(within(proposed).getByRole('button', { name: 'Commit and run' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_commit', { companyId: 'company-1', proposal: 'proposal-5' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_run', { companyId: 'company-1', mapping: 'map-1', offset: 0 }))
    const result = await within(panel).findByRole('group', { name: 'Import result' })
    expect(result).toHaveTextContent('3 rows in customers.csv')
    expect(result).toHaveTextContent('2 created, 0 updated, 0 unchanged')
    expect(result).toHaveTextContent('1 row not placed')
    const queue = within(result).getByRole('table', { name: 'Rows not placed' })
    expect(within(queue).getAllByRole('row')[1]).toHaveTextContent('3No Sitethe Website cell is empty, so the row has no identity')

    recordQueryResult = { page: { kind: 'org', total: 2, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [{ id: 'org-1', kind: 'org', title: 'Northwind', state: null, updated_at: '2026-09-16T10:00:00.000Z', data: { name: 'Northwind', domain: 'northwind.example' } }] } }
    await fireEvent.click(within(result).getByRole('button', { name: 'Done' }))
    await within(panel).findByRole('table', { name: 'org records' })
    expect(await within(panel).findByText('2 records')).toBeInTheDocument()
    // Done rereads the kind list too, so its counts hold the rows the import landed.
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'record_kinds').length).toBeGreaterThanOrEqual(2))
  })

  it('appends the next page with Show more, and rereads the table when a run ends and when the window regains focus', async () => {
    const rows = (from, count) => Array.from({ length: count }, (_, index) => ({ id: `deal-${from + index}`, kind: 'deal', title: `Deal ${from + index}`, state: 'won', updated_at: '2026-09-15T10:30:00.000Z', data: { name: `Deal ${from + index}`, stage: 'won' } }))
    recordQueryResult = { page: { kind: 'deal', total: 260, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: rows(0, 200) } }
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[1])
    await within(panel).findByRole('table', { name: 'deal records' })
    expect(within(panel).getByText('260 records')).toBeInTheDocument()
    const more = within(panel).getByRole('button', { name: 'Show 60 more' })
    recordQueryResult = { page: { kind: 'deal', total: 260, offset: 200, limit: 200, sort: 'updated_at', descending: true, rows: rows(200, 60) } }
    await fireEvent.click(more)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_query', expect.objectContaining({ kind: 'deal', offset: 200, limit: 200 })))
    await waitFor(() => expect(within(panel).getAllByRole('row')).toHaveLength(261))
    expect(within(panel).queryByRole('button', { name: /Show .* more/ })).not.toBeInTheDocument()

    const queries = invoke.mock.calls.filter(([command]) => command === 'record_query').length
    await fireEvent(window, new Event('focus'))
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'record_query').length).toBe(queries + 1))
  })

  it('renames the company from the kind list', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    await openKinds(panel)
    await fireEvent.click(within(panel).getByRole('button', { name: 'Rename' }))
    const name = within(panel).getByRole('textbox', { name: 'Company name' })
    expect(name).toHaveValue('Northwind')
    await fireEvent.input(name, { target: { value: 'Northwind Traders' } })
    await fireEvent.click(within(panel).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_company_rename', { companyId: 'company-1', name: 'Northwind Traders' }))
    const picker = await within(panel).findByRole('combobox', { name: 'Company' })
    expect(within(picker).getByRole('option', { name: 'Northwind Traders' })).toBeInTheDocument()
  })

  it('links, merges and deletes an open record through propose and commit', async () => {
    recordKindsResult = { company_id: 'company-1', kinds: [{ name: 'org', schema: { properties: { name: {} } }, states: null, extension: null }, { name: 'person', schema: { properties: { full_name: {} } }, states: null, extension: null }], relations: [{ name: 'works_at', from: ['person'], to: ['org'] }, { name: 'about', from: ['any'], to: ['any'] }, { name: 'owns', from: ['person'], to: ['org'] }] }
    recordQueryResult = { page: { kind: 'org', total: 2, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [
      { id: 'org-1', kind: 'org', title: 'Northwind', state: null, updated_at: '2026-09-15T10:30:00.000Z', data: { name: 'Northwind' } },
      { id: 'org-2', kind: 'org', title: 'Northwind Inc', state: null, updated_at: '2026-09-14T10:30:00.000Z', data: { name: 'Northwind Inc' } },
    ] } }
    recordEntityResult = { entity: { entity: { id: 'org-2', kind: 'org', title: 'Northwind Inc', state: null, updated_at: '2026-09-14T10:30:00.000Z', body_text: 'Northwind Inc.', data: { name: 'Northwind Inc' } }, kind: recordKindsResult.kinds[0], identities: [], edges: [], events: [] } }
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[0])
    await within(panel).findByRole('table', { name: 'org records' })
    await fireEvent.click(within(panel).getByRole('button', { name: 'Northwind Inc' }))
    await within(panel).findByRole('article', { name: 'Northwind Inc' })

    // Link: the relations the vocabulary allows from an org, a target found by title.
    await fireEvent.click(within(panel).getByRole('button', { name: 'Link' }))
    const link = within(panel).getByRole('form', { name: 'Link Northwind Inc' })
    const relation = within(link).getByRole('combobox', { name: 'Relation' })
    expect(within(relation).getAllByRole('option').map((option) => option.textContent)).toEqual(['about'])
    expect(within(link).getByRole('combobox', { name: 'Target kind' })).toHaveValue('org')
    await fireEvent.input(within(link).getByRole('searchbox', { name: 'Search org' }), { target: { value: 'North' } })
    await fireEvent.click(within(link).getByRole('button', { name: 'Find' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_query', expect.objectContaining({ kind: 'org', search: 'North', limit: 20 })))
    const matches = await within(link).findByRole('list', { name: 'Matches' })
    expect(within(matches).queryByRole('button', { name: /Northwind Inc/ })).not.toBeInTheDocument()
    recordProposeResult = { proposal: { id: 'proposal-6', warnings: [], diff: { op: 'link', link: { relation: 'about' }, src_title: 'Northwind Inc', dst_title: 'Northwind' } } }
    recordCommitResult = { result: { event_seq: 7, entity_ids: ['org-2', 'org-1'] } }
    await fireEvent.click(within(matches).getByRole('button', { name: 'Northwind' }))
    await fireEvent.click(within(link).getByRole('button', { name: 'Propose' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'link', src: 'entity:org-2', relation: 'about', dst: 'entity:org-1' } }))
    const proposedLink = await within(link).findByRole('group', { name: 'Proposed link' })
    expect(proposedLink).toHaveTextContent('about: Northwind Inc to Northwind')
    await fireEvent.click(within(proposedLink).getByRole('button', { name: 'Commit' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_commit', { companyId: 'company-1', proposal: 'proposal-6' }))
    await within(panel).findByRole('article', { name: 'Northwind Inc' })

    // Merge: the survivor is of the same kind, and the panel opens it after the commit.
    await fireEvent.click(within(panel).getByRole('button', { name: 'Merge' }))
    const merge = within(panel).getByRole('form', { name: 'Merge Northwind Inc into' })
    await fireEvent.click(within(merge).getByRole('button', { name: 'Find' }))
    const survivors = await within(merge).findByRole('list', { name: 'Matches' })
    await fireEvent.click(within(survivors).getByRole('button', { name: 'Northwind' }))
    recordProposeResult = { proposal: { id: 'proposal-7', warnings: [], diff: { op: 'merge', loser: { title: 'Northwind Inc' }, survivor: { title: 'Northwind' }, identities_moved: 1 } } }
    recordCommitResult = { result: { event_seq: 8, entity_ids: ['org-1', 'org-2'] } }
    await fireEvent.click(within(merge).getByRole('button', { name: 'Propose' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'merge', loser: 'entity:org-2', survivor: 'entity:org-1' } }))
    const proposedMerge = await within(merge).findByRole('group', { name: 'Proposed merge' })
    expect(proposedMerge).toHaveTextContent('Northwind Inc into Northwind, 1 identity move')
    recordEntityResult = { entity: { entity: { id: 'org-1', kind: 'org', title: 'Northwind', state: null, updated_at: '2026-09-15T10:30:00.000Z', body_text: 'Northwind.', data: { name: 'Northwind' } }, kind: recordKindsResult.kinds[0], identities: [], edges: [], events: [] } }
    await fireEvent.click(within(proposedMerge).getByRole('button', { name: 'Commit' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_entity', { companyId: 'company-1', entity: 'org-1' }))
    await within(panel).findByRole('article', { name: 'Northwind' })

    // Delete: one sentence, the diff, and the table without the row.
    await fireEvent.click(within(panel).getByRole('button', { name: 'Delete' }))
    const remove = within(panel).getByRole('form', { name: 'Delete Northwind' })
    recordProposeResult = { proposal: { id: 'proposal-8', warnings: [], diff: { op: 'delete', before: { kind: 'org', title: 'Northwind' } } } }
    recordCommitResult = { result: { event_seq: 9, entity_ids: ['org-1'] } }
    await fireEvent.click(within(remove).getByRole('button', { name: 'Propose' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'delete', entity: 'entity:org-1' } }))
    const proposedDelete = await within(remove).findByRole('group', { name: 'Proposed delete' })
    expect(proposedDelete).toHaveTextContent('org Northwind is deleted')
    recordQueryResult = { page: { kind: 'org', total: 0, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [] } }
    await fireEvent.click(within(proposedDelete).getByRole('button', { name: 'Commit' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_commit', { companyId: 'company-1', proposal: 'proposal-8' }))
    await within(panel).findByText('No org records yet')
  })

  it('connects Stripe once, picks an object and maps it with the source id as the key', async () => {
    recordKindsResult = { company_id: 'company-1', kinds: [{ name: 'org', schema: { properties: { name: { type: 'string' }, domain: { type: 'string' } }, required: ['name'] }, states: null, extension: null }] }
    recordQueryResult = { page: { kind: 'org', total: 0, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [] } }
    readerDescribeResult = { description: { source: 'stripe', object: 'customers', label: 'Customers', rows: 100, counted: false, bytes: 0, hash: '1700000100', fields: [
      { name: 'id', guess: 'id', samples: ['cus_1', 'cus_2'], filled: 100 },
      { name: 'name', guess: 'string', samples: ['Northwind'], filled: 98 },
      { name: 'email_domain', guess: 'domain', samples: ['northwind.example'], filled: 80 },
    ] } }
    recordProposeResult = { proposal: { id: 'proposal-9', warnings: [], diff: { op: 'create', after: { id: 'map-2', data: {} } } } }
    recordCommitResult = { result: { event_seq: 10, entity_ids: ['map-2'] } }
    readerRunResults = [{ run: { mapping: 'map-2', source: 'stripe', object: 'customers', label: 'Customers', kind: 'org', offset: 0, next_offset: 100, done: true, total: 100, counted: false, changed: true, created: 100, updated: 0, unchanged: 0, unplaced: 0, queue: [] } }]
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[0])
    await within(panel).findByText('No org records yet')
    await fireEvent.click(within(panel).getByRole('button', { name: 'Import' }))
    await fireEvent.click(within(within(panel).getByRole('list', { name: 'Sources' })).getByRole('button', { name: /Stripe/ }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_objects', { companyId: 'company-1', source: 'stripe' }))
    const connect = await within(panel).findByRole('form', { name: 'Connect Stripe' })
    expect(connect).toHaveTextContent('It stays in this machine\'s keychain')
    const key = within(connect).getByLabelText('Secret key')
    expect(key).toHaveAttribute('type', 'password')
    await fireEvent.input(key, { target: { value: 'sk_live_x' } })
    await fireEvent.click(within(connect).getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_connect', { companyId: 'company-1', source: 'stripe', secret: 'sk_live_x' }))
    const objects = await within(panel).findByRole('list', { name: 'Objects' })
    expect(within(objects).getAllByRole('button').map((button) => button.textContent)).toEqual(['Customers', 'Subscriptions', 'Invoices'])
    await fireEvent.click(within(objects).getByRole('button', { name: 'Customers' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_describe', { companyId: 'company-1', source: 'stripe', object: 'customers' }))
    const form = await within(panel).findByRole('form', { name: 'Map Customers' })
    expect(form).toHaveTextContent('Customers · 100 rows read so far')
    expect(within(form).getByRole('combobox', { name: 'Property for name' })).toHaveValue('name')
    expect(within(form).getByRole('combobox', { name: 'Property for email_domain' })).toHaveValue('domain')
    const identity = within(form).getByRole('combobox', { name: 'Identity' })
    expect(identity).toHaveValue('external:stripe:customers:id')
    await fireEvent.click(within(form).getByRole('button', { name: 'Propose mapping' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'create', kind: 'mapping', data: { source: 'stripe', object: 'customers', kind: 'org', fields: { name: 'name', email_domain: 'domain' }, approved: true, identity: 'external:stripe:customers:id' } } }))
    const proposed = await within(form).findByRole('group', { name: 'Proposed mapping' })
    expect(proposed).toHaveTextContent('keyed on external in id')
    expect(proposed).toHaveTextContent('stripe Customers')
    await fireEvent.click(within(proposed).getByRole('button', { name: 'Commit and run' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_run', { companyId: 'company-1', mapping: 'map-2', offset: 0 }))
    const result = await within(panel).findByRole('group', { name: 'Import result' })
    expect(result).toHaveTextContent('100 rows in Customers')
    expect(result).toHaveTextContent('100 created, 0 updated, 0 unchanged')
  })

  it('runs a mapping record again from its own view', async () => {
    recordKindsResult = { company_id: 'company-1', kinds: [{ name: 'mapping', schema: { properties: { source: { type: 'string' }, object: { type: 'string' }, kind: { type: 'string' }, fields: { type: 'object' }, identity: { type: 'string' }, approved: { type: 'boolean' }, cursors: { type: 'object' } }, required: ['source', 'object', 'kind'] }, states: null, extension: null }, { name: 'org', schema: { properties: { name: { type: 'string' } } }, states: null, extension: null }] }
    recordQueryResult = { page: { kind: 'mapping', total: 1, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [{ id: 'map-1', kind: 'mapping', title: 'csv customers.csv to org', state: null, updated_at: '2026-09-16T10:00:00.000Z', data: { source: 'csv', object: '/exports/customers.csv', kind: 'org', approved: true } }] } }
    recordEntityResult = { entity: { entity: { id: 'map-1', kind: 'mapping', title: 'csv customers.csv to org', state: null, updated_at: '2026-09-16T10:00:00.000Z', body_text: 'Mapping of csv customers.csv onto org.', data: { source: 'csv', object: '/exports/customers.csv', kind: 'org', fields: { Company: 'name' }, approved: true } }, kind: recordKindsResult.kinds[0], identities: [], edges: [], events: [] } }
    readerRunResults = [
      { run: { mapping: 'map-1', label: 'customers.csv', kind: 'org', offset: 0, next_offset: 200, done: false, total: 260, changed: true, created: 200, updated: 0, unchanged: 0, unplaced: 0, queue: [] } },
      { run: { mapping: 'map-1', label: 'customers.csv', kind: 'org', offset: 200, next_offset: 260, done: true, total: 260, changed: true, created: 0, updated: 0, unchanged: 60, unplaced: 0, queue: [] } },
    ]
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[0])
    await within(panel).findByRole('table', { name: 'mapping records' })
    await fireEvent.click(within(panel).getByRole('button', { name: 'csv customers.csv to org' }))
    await within(panel).findByRole('article', { name: 'csv customers.csv to org' })
    await fireEvent.click(within(panel).getByRole('button', { name: 'Run' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_run', { companyId: 'company-1', mapping: 'map-1', offset: 0 }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('reader_run', { companyId: 'company-1', mapping: 'map-1', offset: 200 }))
    const result = await within(panel).findByRole('group', { name: 'Import result' })
    expect(result).toHaveTextContent('260 rows in customers.csv')
    expect(result).toHaveTextContent('200 created, 0 updated, 60 unchanged')
    expect(within(result).queryByRole('table', { name: 'Rows not placed' })).not.toBeInTheDocument()
    await fireEvent.click(within(result).getByRole('button', { name: 'Done' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_entity', { companyId: 'company-1', entity: 'map-1' }))
  })

  it('switches a kind with states to the board, moves a card through propose and commit, saves a view and asks about it', async () => {
    recordQueryResult = { page: { kind: 'deal', total: 2, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [
      { id: 'deal-1', kind: 'deal', title: 'Northwind renewal', state: 'won', updated_at: '2026-09-15T10:30:00.000Z', created_at: '2026-09-15T10:00:00.000Z', data: { name: 'Northwind renewal', stage: 'won' } },
      { id: 'deal-2', kind: 'deal', title: 'Contoso pilot', state: 'discovery', updated_at: '2026-09-14T10:30:00.000Z', created_at: '2026-09-14T10:00:00.000Z', data: { name: 'Contoso pilot', stage: 'discovery' } },
    ] } }
    recordProposeResult = { proposal: { id: 'proposal-3', warnings: [], diff: { op: 'update', before: { data: { name: 'Contoso pilot', stage: 'discovery' } }, after: { data: { name: 'Contoso pilot', stage: 'won' } } } } }
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })
    const kinds = await openKinds(panel)
    await fireEvent.click(within(kinds).getAllByRole('button')[1])
    await within(panel).findByRole('table', { name: 'deal records' })
    const toolbar = within(panel).getByRole('toolbar', { name: 'View' })
    expect(within(toolbar).getByRole('button', { name: 'Table' })).toHaveAttribute('aria-pressed', 'true')

    await fireEvent.click(within(toolbar).getByRole('button', { name: 'Board' }))
    const board = await within(panel).findByRole('region', { name: 'deal board' })
    expect(within(panel).queryByRole('table')).not.toBeInTheDocument()
    const discovery = within(board).getByRole('region', { name: 'Discovery' })
    const won = within(board).getByRole('region', { name: 'Won' })
    expect(within(discovery).getByRole('button', { name: 'Contoso pilot' })).toBeInTheDocument()
    expect(within(won).getByRole('button', { name: 'Northwind renewal' })).toBeInTheDocument()

    const card = within(discovery).getByRole('button', { name: 'Contoso pilot' }).closest('.record-card')
    await fireEvent.pointerDown(card, { button: 0, clientX: 10, clientY: 10 })
    await fireEvent.pointerMove(won, { clientX: 240, clientY: 12 })
    expect(won).toHaveClass('over')
    expect(card).toHaveClass('dragging')
    await fireEvent.pointerUp(won, { clientX: 240, clientY: 12 })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'update', entity: 'entity:deal-2', data: { stage: 'won' } } }))
    const move = await within(panel).findByRole('group', { name: 'Proposed move' })
    expect(move).toHaveTextContent('stage: discovery to won')
    await fireEvent.click(within(move).getByRole('button', { name: 'Commit' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_commit', { companyId: 'company-1', proposal: 'proposal-3' }))
    await waitFor(() => expect(within(panel).queryByRole('group', { name: 'Proposed move' })).not.toBeInTheDocument())

    await fireEvent.change(within(toolbar).getByRole('combobox', { name: 'State' }), { target: { value: 'won' } })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_query', expect.objectContaining({ kind: 'deal', state: 'won' })))

    recordProposeResult = { proposal: { id: 'proposal-4', warnings: [], diff: { op: 'create', after: { data: { name: 'Won deals', kind: 'deal', layout: 'board' } } } } }
    recordCommitResult = { result: { event_seq: 5, entity_ids: ['view-1'] } }
    await fireEvent.click(within(toolbar).getByRole('button', { name: 'Save view' }))
    const save = within(panel).getByRole('form', { name: 'Save view' })
    await fireEvent.input(within(save).getByRole('textbox', { name: 'View name' }), { target: { value: 'Won deals' } })
    await fireEvent.click(within(save).getByRole('button', { name: 'Propose' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_propose', { companyId: 'company-1', operation: { op: 'create', kind: 'view', data: { name: 'Won deals', kind: 'deal', layout: 'board', sort: [{ column: 'updated_at', descending: true }], filters: [{ property: 'state', equals: 'won' }] } } }))
    const proposedView = await within(panel).findByRole('group', { name: 'Proposed view' })
    expect(proposedView).toHaveTextContent('layout: board')
    recordQueryResult = { page: { kind: 'view', total: 1, offset: 0, limit: 200, sort: 'title', descending: false, rows: [{ id: 'view-1', kind: 'view', title: 'Won deals', state: null, updated_at: '2026-09-15T11:00:00.000Z', data: { name: 'Won deals', kind: 'deal', layout: 'board', sort: [{ column: 'updated_at', descending: true }], filters: [{ property: 'state', equals: 'won' }] } }] } }
    await fireEvent.click(within(proposedView).getByRole('button', { name: 'Commit' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_commit', { companyId: 'company-1', proposal: 'proposal-4' }))
    const picker = await within(toolbar).findByRole('combobox', { name: 'Saved view' })
    expect(within(picker).getByRole('option', { name: 'Won deals' })).toBeInTheDocument()
    await waitFor(() => expect(picker).toHaveValue('view-1'))

    await fireEvent.click(within(toolbar).getByRole('button', { name: 'Ask' }))
    const composer = await findWorkspaceComposer()
    expect(composer.value).toBe('```sql\nselect "title", "state", "updated_at", "name"\nfrom "v_deal"\nwhere state = \'won\'\norder by "updated_at" desc\nlimit 200\n```\n')
  })

  it('toggles with the platform shortcut, maximizes over the sidebar and thread, and restores with the shortcut then closes with Escape', async () => {
    render(App)
    const record = await screen.findByRole('button', { name: 'Open record panel' })
    const mac = navigator.platform.startsWith('Mac')
    await fireEvent.keyDown(document, { key: 'k', metaKey: mac, ctrlKey: !mac })
    expect(record).toHaveAttribute('aria-expanded', 'true')
    const workspace = document.querySelector('.workspace')
    expect(workspace).toHaveClass('rail-open')
    expect(screen.getByRole('separator', { name: 'Record' })).toHaveAttribute('aria-valuemin', '480')

    const maximize = screen.getByRole('button', { name: 'Maximize the record over the thread' })
    await fireEvent.click(maximize)
    expect(workspace).toHaveClass('record-maximized')
    expect(screen.queryByRole('separator', { name: 'Record' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Restore the thread beside the record' })).toHaveAttribute('aria-pressed', 'true')

    await fireEvent.keyDown(document, { key: 'k', metaKey: mac, ctrlKey: !mac })
    expect(workspace).not.toHaveClass('record-maximized')
    expect(record).toHaveAttribute('aria-expanded', 'true')

    await fireEvent.keyDown(document, { key: 'Escape' })
    expect(record).toHaveAttribute('aria-expanded', 'false')
    expect(workspace).not.toHaveClass('rail-open')
  })

  it('offers to create the first company and names a record failure in one line', async () => {
    recordCompaniesResult = { companies: [], current: null }
    render(App)
    const record = await screen.findByRole('button', { name: 'Open record panel' })
    await fireEvent.click(record)
    const panel = screen.getByRole('complementary', { name: 'Record' })
    await within(panel).findByText('Your company, in one record.')
    expect(within(panel).getByText('Northwind Traders, a sample company · 8 open findings · 3 fewer than the last run')).toBeInTheDocument()
    expect(within(panel).getByText('4,182')).toBeInTheDocument()
    expect(within(panel).getByText('people appear more than once across HubSpot and Salesforce, so every owner report and every campaign count reads them twice.')).toBeInTheDocument()
    expect(within(within(panel).getByRole('list', { name: 'Sources' })).getAllByRole('button')).toHaveLength(29)
    const name = within(panel).getByRole('textbox', { name: 'Company name' })
    const create = within(panel).getByRole('button', { name: 'Create your company' })
    expect(create).toBeDisabled()
    await fireEvent.input(name, { target: { value: 'Northwind' } })
    expect(create).toBeEnabled()
    await fireEvent.click(create)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('record_company_create', { name: 'Northwind' }))
    await within(panel).findByRole('combobox', { name: 'Company' })
    await openKinds(panel)

    recordKindsResult = { error: { code: 'unknown_company', message: 'No company has id x. Pick another one.' } }
    await fireEvent.click(record)
    await fireEvent.click(record)
    await screen.findByRole('alert')
    expect(screen.getByRole('alert')).toHaveTextContent('No company has id x.')
  })

  it('opens the sample company and takes the first source with the company name', async () => {
    recordCompaniesResult = { companies: [], current: null }
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open record panel' }))
    const panel = screen.getByRole('complementary', { name: 'Record' })

    await fireEvent.click(await within(panel).findByRole('button', { name: 'Open the sample' }))
    const sample = await within(panel).findByRole('region', { name: 'Northwind Traders sample' })
    expect(within(sample).getByText('Northwind Traders · 8 open findings · 3 fewer than the last run')).toBeInTheDocument()
    const review = within(sample).getAllByRole('button', { name: 'Review' })[0]
    expect(review).toHaveAttribute('aria-expanded', 'false')
    await fireEvent.click(review)
    expect(within(sample).getByText('rule lower(email) equal, source ids differ')).toBeInTheDocument()
    await fireEvent.click(within(sample).getByRole('button', { name: 'Close the sample' }))

    const stripe = await within(panel).findByRole('button', { name: /^Stripe/ })
    await fireEvent.click(stripe)
    expect(stripe).toHaveAttribute('aria-pressed', 'true')
    await fireEvent.input(within(panel).getByRole('textbox', { name: 'Company name' }), { target: { value: 'Northwind' } })
    expect(within(panel).getByRole('button', { name: 'Create your company and connect Stripe' })).toBeEnabled()
  })

})

describe('artifact rail', () => {
  it('uses one accessible heading for the rail', () => {
    const railMarkup = appSource.match(/<aside id="artifact-rail"[\s\S]*?<\/aside>/)?.[0] ?? ''

    expect(railMarkup).toMatch(/aria-labelledby="artifact-rail-title"/)
    expect(railMarkup.match(/<h2 id="artifact-rail-title">Artifacts<\/h2>/g)).toHaveLength(1)
    expect(railMarkup).not.toMatch(/Thread artifacts/)
  })

  it('toggles from the titlebar button with accessible state and an honest empty landmark', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    expect(toggle).toHaveAttribute('aria-controls', 'artifact-rail')
    expect(toggle).toHaveAttribute('aria-keyshortcuts', navigator.platform.startsWith('Mac') ? 'Meta+J' : 'Control+J')
    expect(within(toggle).getByText('Artifacts')).toBeInTheDocument()
    expect(within(toggle).getByText(navigator.platform.startsWith('Mac') ? '⌘ J' : 'Ctrl J').tagName).toBe('KBD')
    expect(screen.queryByRole('complementary', { name: 'Artifacts' })).not.toBeInTheDocument()

    await fireEvent.click(toggle)

    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(toggle).toHaveAccessibleName('Close artifact rail')
    const rail = screen.getByRole('complementary', { name: 'Artifacts' })
    const empty = rail.querySelector('.artifact-empty')
    expect(empty).toHaveTextContent('Ask in chat to create a document, table, or other file.')
    expect(empty.children).toHaveLength(1)
    expect(within(empty).getByText('Ask in chat to create a document, table, or other file.').tagName).toBe('P')

    await fireEvent.click(toggle)
    expect(screen.queryByRole('complementary', { name: 'Artifacts' })).not.toBeInTheDocument()
  })

  it('toggles with the platform keyboard shortcut and closes with Escape', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    const mac = navigator.platform.startsWith('Mac')
    await fireEvent.keyDown(document, { key: 'j', metaKey: mac, ctrlKey: !mac })
    expect(toggle).toHaveAttribute('aria-expanded', 'true')

    await fireEvent.keyDown(document, { key: 'Escape' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
  })

  it('exposes an operable window splitter and resets its width after closing', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    await fireEvent.click(toggle)
    const separator = screen.getByRole('separator', { name: 'Artifacts' })
    expect(separator).toHaveAttribute('tabindex', '0')
    expect(separator).toHaveAttribute('aria-orientation', 'vertical')
    expect(separator).toHaveAttribute('aria-valuemin', '380')
    // 1024 wide, the 195px sidebar and the 320px thread minimum leave 509px.
    expect(separator).toHaveAttribute('aria-valuemax', '509')
    expect(separator).toHaveAttribute('aria-valuenow', '380')

    await fireEvent.keyDown(separator, { key: 'ArrowLeft' })
    expect(separator).toHaveAttribute('aria-valuenow', '400')
    await fireEvent.keyDown(separator, { key: 'ArrowRight' })
    expect(separator).toHaveAttribute('aria-valuenow', '380')
    await fireEvent.keyDown(separator, { key: 'End' })
    expect(separator).toHaveAttribute('aria-valuenow', separator.getAttribute('aria-valuemax'))
    await fireEvent.keyDown(separator, { key: 'Home' })
    expect(separator).toHaveAttribute('aria-valuenow', '380')

    await fireEvent.keyDown(separator, { key: 'ArrowLeft' })
    await fireEvent.click(toggle)
    await fireEvent.click(toggle)
    expect(screen.getByRole('separator', { name: 'Artifacts' })).toHaveAttribute('aria-valuenow', '380')
  })

  it('reserves the paper frame when sizing and dragging the artifact rail', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    const workspace = toggle.closest('.workspace')
    workspace.style.paddingRight = '8px'
    vi.spyOn(workspace, 'getBoundingClientRect').mockReturnValue({ right: 1024 })

    await fireEvent.click(toggle)
    const separator = screen.getByRole('separator', { name: 'Artifacts' })
    expect(separator).toHaveAttribute('aria-valuemax', '477')
    await fireEvent.keyDown(separator, { key: 'End' })
    expect(separator).toHaveAttribute('aria-valuenow', '477')

    await fireEvent.pointerDown(separator, { button: 0, pointerId: 7, clientX: 604 })
    await fireEvent.pointerMove(separator, { pointerId: 7, clientX: 616 })
    expect(separator).toHaveAttribute('aria-valuenow', '400')
    await fireEvent.pointerMove(separator, { pointerId: 7, clientX: 0 })
    expect(separator).toHaveAttribute('aria-valuenow', '477')
    await fireEvent.pointerMove(separator, { pointerId: 7, clientX: 1024 })
    expect(separator).toHaveAttribute('aria-valuenow', '380')
    await fireEvent.pointerUp(separator, { pointerId: 7 })
  })

  it('resizes the sidebar from its divider with the pointer and the keyboard, and keeps the width', async () => {
    render(App)
    await screen.findByRole('button', { name: 'Collapse sidebar' })
    const separator = screen.getByRole('separator', { name: 'Threads' })
    const workspace = separator.closest('.workspace')
    vi.spyOn(workspace, 'getBoundingClientRect').mockReturnValue({ left: 0, right: 1024 })
    expect(separator).toHaveAttribute('tabindex', '0')
    expect(separator).toHaveAttribute('aria-orientation', 'vertical')
    expect(separator).toHaveAttribute('aria-controls', 'sidebar')
    expect(separator).toHaveAttribute('aria-valuemin', '160')
    expect(separator).toHaveAttribute('aria-valuenow', '195')
    expect(workspace.style.getPropertyValue('--sidebar-column')).toBe('195px')

    await fireEvent.keyDown(separator, { key: 'ArrowRight' })
    expect(separator).toHaveAttribute('aria-valuenow', '215')
    expect(workspace.style.getPropertyValue('--sidebar-column')).toBe('215px')
    await fireEvent.keyDown(separator, { key: 'Home' })
    expect(separator).toHaveAttribute('aria-valuenow', '160')

    await fireEvent.pointerDown(separator, { button: 0, pointerId: 5, clientX: 160 })
    expect(workspace.classList.contains('sidebar-resizing')).toBe(true)
    await fireEvent.pointerMove(separator, { pointerId: 5, clientX: 240 })
    expect(separator).toHaveAttribute('aria-valuenow', '240')
    await fireEvent.pointerUp(separator, { pointerId: 5 })
    expect(workspace.classList.contains('sidebar-resizing')).toBe(false)
    await fireEvent.pointerMove(separator, { pointerId: 5, clientX: 300 })
    expect(separator).toHaveAttribute('aria-valuenow', '240')
    expect(localStorage.getItem('muniment.sidebar-width')).toBe('240')

    await fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }))
    expect(screen.queryByRole('separator', { name: 'Threads' })).not.toBeInTheDocument()
    expect(workspace.style.getPropertyValue('--sidebar-column')).toBe('0px')
    await fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }))
    expect(screen.getByRole('separator', { name: 'Threads' })).toHaveAttribute('aria-valuenow', '240')
    expect(appRules.get('.artifact-divider, .sidebar-divider')).toMatch(/cursor:\s*col-resize/)
    expect(appStyles).not.toMatch(/divider[^{]*::after/)
  })

  it('finishes pointer resizing on release and cancellation', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open artifact rail' }))
    const separator = screen.getByRole('separator', { name: 'Artifacts' })
    const workspace = separator.closest('.workspace')
    vi.spyOn(workspace, 'getBoundingClientRect').mockReturnValue({ right: 1024 })

    await fireEvent.pointerDown(separator, { button: 0, pointerId: 7, clientX: 624 })
    await fireEvent.pointerMove(separator, { pointerId: 7, clientX: 604 })
    expect(separator).toHaveAttribute('aria-valuenow', '420')
    await fireEvent.pointerUp(separator, { pointerId: 7 })
    await fireEvent.pointerMove(separator, { pointerId: 7, clientX: 584 })
    expect(separator).toHaveAttribute('aria-valuenow', '420')

    await fireEvent.pointerDown(separator, { button: 0, pointerId: 8, clientX: 604 })
    await fireEvent.pointerMove(separator, { pointerId: 8, clientX: 594 })
    expect(separator).toHaveAttribute('aria-valuenow', '430')
    await fireEvent.pointerCancel(separator, { pointerId: 8 })
    await fireEvent.pointerMove(separator, { pointerId: 8, clientX: 584 })
    expect(separator).toHaveAttribute('aria-valuenow', '430')
  })

  it('toggles the artifact rail and sidebar from the focused composer', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    const composer = screen.getByPlaceholderText('Ask anything')
    const mac = navigator.platform.startsWith('Mac')
    expect(composer).toHaveFocus()

    await fireEvent.keyDown(composer, { key: 'j', metaKey: mac, ctrlKey: !mac })
    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('complementary', { name: 'Artifacts' })).toBeInTheDocument()

    expect(composer).toHaveFocus()
    await fireEvent.keyDown(composer, { key: '\\', metaKey: mac, ctrlKey: !mac })
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
  })

  it('closes with Escape while active dictation is also cancelled', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'Original draft' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Voice' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Voice' })).toHaveAttribute('aria-pressed', 'true'))
    await fireEvent.click(screen.getByRole('button', { name: 'Open artifact rail' }))

    await fireEvent.keyDown(document, { key: 'Escape' })

    expect(screen.getByRole('button', { name: 'Open artifact rail' })).toHaveAttribute('aria-expanded', 'false')
    expect(composer).toHaveValue('Original draft')
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_stop'))
    expect(composer).toHaveFocus()
  })

  it('cannot prime or retain an open rail outside the signed-in workspace', async () => {
    let signedIn = false
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: signedIn, subject: signedIn ? 'user-a' : null }
      if (command === 'auth_sign_in') {
        signedIn = true
        return { signed_in: true, subject: 'user-a' }
      }
      if (command === 'auth_sign_out') {
        signedIn = false
        return { signed_in: false, subject: null }
      }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const mac = navigator.platform.startsWith('Mac')
    const shortcut = { key: 'j', metaKey: mac, ctrlKey: !mac }
    const signIn = await screen.findByRole('button', { name: 'Sign in' })
    await fireEvent.keyDown(document, shortcut)
    await fireEvent.click(signIn)

    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    await fireEvent.click(toggle)
    await fireEvent.click(screen.getByRole('button', { name: /Alice/i }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Sign out' }))
    await screen.findByRole('button', { name: 'Sign in' })
    await fireEvent.keyDown(document, shortcut)
    await fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(await screen.findByRole('button', { name: 'Open artifact rail' })).toHaveAttribute('aria-expanded', 'false')
  })

  it('ignores the rail shortcut during signed-in onboarding', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    render(App)
    await screen.findByRole('textbox', { name: 'Message' })
    const mac = navigator.platform.startsWith('Mac')
    const shortcut = new KeyboardEvent('keydown', { key: 'j', metaKey: mac, ctrlKey: !mac, cancelable: true })

    document.dispatchEvent(shortcut)

    expect(shortcut.defaultPrevented).toBe(false)
    expect(screen.queryByRole('complementary', { name: 'Artifacts' })).not.toBeInTheDocument()
  })
})

describe('message grammar', () => {
  it('caps user bubbles while preserving compact right alignment and long-token wrapping', () => {
    expect(appRules.get('.user-message')).toMatch(/width:\s*fit-content/)
    expect(appRules.get('.user-message')).toMatch(/max-width:\s*78%/)
    expect(appRules.get('.user-message')).toMatch(/margin-left:\s*auto/)
    expect(appRules.get('.user-message')).toMatch(/overflow-wrap:\s*anywhere/)
  })
})

describe('history alerts', () => {
  it('wraps an unbroken reader cause inside the alert', () => {
    expect(appRules.get('.history-error')).toMatch(/overflow-wrap:\s*anywhere/)
  })

  it('shows no history alert for an empty journal in local mode', async () => {
    localModeStatus = true
    threadSummaryResult = []
    const base = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => command === 'local_mode_provider_inventory' ? Promise.resolve({ providers: [], hidden: [] }) : base(command, payload))
    render(App)

    await screen.findByText('Connect a model in the composer to start chatting.')

    expect(document.querySelector('.history-error')).not.toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', expect.anything())
  })

  it('explains an empty router account pool instead of advertising the selected classifier', async () => {
    localModeStatus = true
    threadSummaryResult = []
    const base = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => command === 'local_mode_provider_inventory' ? Promise.resolve({
      providers: [{ id: 'muniment-router', name: 'Router', source: 'router', models: [] }],
      default_provider: 'muniment-router', default_model: 'auto', router_classifier: 'classifier',
      router_models: [{ id: 'openai/model-a', family: 'openai', model: 'model-a', accounts: 0 }],
    }) : base(command, payload))
    render(App)
    await screen.findByText('No enabled account serves this model. Connect an account or choose another model in the composer.')
    expect(document.querySelector('.empty')).not.toHaveTextContent('is selected')
  })

  it('shows an inventory read error instead of a model readiness claim', async () => {
    localModeStatus = true
    threadSummaryResult = []
    const base = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => command === 'local_mode_provider_inventory' ? Promise.reject(new Error('unavailable')) : base(command, payload))
    render(App)
    await screen.findByText('Muniment cannot read provider settings. Restart the app to retry.')
    expect(document.querySelector('.empty')).not.toHaveTextContent('is selected')
  })

  it.each(['Restore history', 'New thread'])('clears the reader cause after %s succeeds', async (action) => {
    let failing = true
    const defaultInvoke = invoke.getMockImplementation()
    invoke.mockImplementation((command, ...args) => {
      if (command === 'chat_thread_open' && failing) return Promise.reject('Cannot read <journal>: permission denied.')
      if (command === 'chat_new_thread') return Promise.resolve()
      return defaultInvoke(command, ...args)
    })
    render(App)

    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('Muniment could not restore conversation history. Cannot read <journal>: permission denied.')
    expect(alert.querySelector('journal')).toBeNull()
    expect(within(alert).getByRole('button', { name: 'Restore history' })).toBeEnabled()

    failing = false
    await fireEvent.click(screen.getByRole('button', { name: action, exact: true }))

    await waitFor(() => expect(document.querySelector('.history-error')).not.toBeInTheDocument())
  })
})

describe('thread name', () => {
  it('exposes the workspace heading, thread list, and named transcript region', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' },
      { threadId: 'thread-2', title: 'Archive review', updatedAt: '' },
    ]
    render(App)

    const heading = await screen.findByRole('heading', { level: 1, name: 'Lease renewal' })
    const listHeading = screen.queryByRole('heading', { name: 'Threads' })
    const list = screen.getByRole('list', { name: 'Threads' })

    expect(heading).toContainElement(screen.getByRole('button', { name: 'Rename thread' }))
    expect(listHeading).not.toBeInTheDocument()
    expect(within(list).getAllByRole('listitem')).toHaveLength(2)
    expect(screen.getByRole('region', { name: 'Transcript: Lease renewal' })).toBeInTheDocument()
  })

  it('appends older threads, moves focus, and removes the control on the last page', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    olderThreadSummaryResult = [{ threadId: 'thread-2', title: 'Older review', updatedAt: '' }]
    render(App)

    const control = await screen.findByRole('button', { name: 'Older threads' })
    const list = screen.getByRole('list', { name: 'Threads' })
    const homeSettings = screen.getByRole('button', { name: 'Settings' })

    expect(within(list).getAllByRole('listitem')).toHaveLength(1)
    expect(within(list).queryByRole('button', { name: 'Older threads' })).not.toBeInTheDocument()
    expect(list.compareDocumentPosition(control)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
    expect(control.compareDocumentPosition(homeSettings)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
    await fireEvent.click(control)

    const olderThread = await screen.findByRole('button', { name: 'Older review' })
    expect(olderThread).toHaveFocus()
    expect(screen.queryByRole('button', { name: 'Older threads' })).not.toBeInTheDocument()
  })

  it('creates and renames a project through folder commands and scopes new threads', async () => {
    const catalog = { projects: { research: 'Research' }, threads: { 'thread-1': 'research' } }
    const baseInvoke = invoke.getMockImplementation()
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'project_list') return structuredClone(catalog)
      if (command === 'project_create') { catalog.projects.created = payload.name; return 'created' }
      if (command === 'project_rename') { catalog.projects[payload.projectId] = payload.name; return }
      if (command === 'project_open') return
      if (command === 'chat_new_thread') { catalog.threads['thread-created'] = payload.projectId; return 'thread-created' }
      return baseInvoke(command, payload)
    })
    render(App)
    await screen.findByRole('button', { name: 'Project Research' })
    await fireEvent.click(screen.getByRole('button', { name: 'New project' }))
    await fireEvent.input(screen.getByRole('textbox', { name: 'Project name' }), { target: { value: 'Contracts' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_new_thread', { projectId: 'created' }))
    expect(await screen.findByRole('button', { name: 'Project Contracts' })).toHaveAttribute('aria-pressed', 'true')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Rename project Contracts' })).toBeEnabled())
    await fireEvent.click(screen.getByRole('button', { name: 'Rename project Contracts' }))
    await fireEvent.input(screen.getByRole('textbox', { name: 'Project name' }), { target: { value: 'Agreements' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await screen.findByRole('button', { name: 'Project Agreements' })
    expect(invoke).toHaveBeenCalledWith('project_rename', { projectId: 'created', name: 'Agreements' })
    await fireEvent.click(screen.getByRole('button', { name: 'Open Agreements folder' }))
    expect(invoke).toHaveBeenCalledWith('project_open', { projectId: 'created' })
  })

  it('places New thread first and keeps pin and archive choices across remounts', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' },
      { threadId: 'thread-2', title: 'Vendor audit', updatedAt: '' },
    ]
    const app = render(App)
    await screen.findByRole('button', { name: 'Vendor audit' })
    expect(document.querySelector('#sidebar button')).toHaveAccessibleName('New thread')
    await fireEvent.click(screen.getByRole('button', { name: 'Actions for Vendor audit' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Pin' }))
    expect(screen.getByRole('heading', { name: 'Pinned' })).toBeInTheDocument()
    expect(document.querySelector('.thread-row-title')).toHaveTextContent('Vendor audit')
    app.unmount()
    render(App)
    await screen.findByRole('heading', { name: 'Pinned' })
    await fireEvent.click(screen.getByRole('button', { name: 'Actions for Vendor audit' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Archive' }))
    expect(screen.queryByRole('heading', { name: 'Pinned' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Vendor audit' })).not.toBeInTheDocument()
    await fireEvent.input(screen.getByRole('searchbox', { name: 'Search threads' }), { target: { value: 'vendor' } })
    expect(screen.getByRole('heading', { name: 'Archived' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Actions for Vendor audit' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Restore' }))
    expect(screen.getByRole('heading', { name: 'Pinned' })).toBeInTheDocument()
  })

  it('searches older titles and renames a thread without opening it', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    olderThreadSummaryResult = [{ threadId: 'thread-2', title: 'Vendor audit', updatedAt: '' }]
    const baseInvoke = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => command === 'chat_rename_thread' ? Promise.resolve() : baseInvoke(command, payload))
    render(App)
    await screen.findByText('Lease renewal', { selector: '.thread-row-title' })
    await fireEvent.input(screen.getByRole('searchbox', { name: 'Search threads' }), { target: { value: 'vendor' } })
    await screen.findByRole('button', { name: 'Vendor audit' })
    expect(screen.getByRole('searchbox')).toHaveValue('vendor')
    await fireEvent.click(screen.getByRole('button', { name: 'Actions for Vendor audit' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
    await fireEvent.input(screen.getByRole('textbox', { name: 'Thread name' }), { target: { value: 'Vendor terms' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_rename_thread', { threadId: 'thread-2', title: 'Vendor terms' }))
    expect(await screen.findByRole('button', { name: 'Vendor terms' })).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('chat_thread_select', { threadId: 'thread-2' })
  })

  it('opens a menu on right-click with the delete action and cancels its inline confirmation', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' },
      { threadId: 'thread-2', title: 'Vendor audit', updatedAt: '' },
    ]
    render(App)
    const row = await screen.findByRole('button', { name: 'Vendor audit' })

    // The row carries a delete control that shows on hover and focus, and the right-click menu is the second path.
    const hoverDelete = screen.getByRole('button', { name: 'Actions for Vendor audit' })
    expect(appStyles).toMatch(/\.thread-record:hover \.thread-actions/)
    await waitFor(() => expect(hoverDelete).toBeEnabled())
    await fireEvent.click(hoverDelete)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete Vendor audit' }))
    expect(within(screen.getByLabelText('Delete Vendor audit?')).getAllByRole('button').map((button) => button.textContent)).toEqual(['Delete', 'Cancel'])
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await fireEvent.contextMenu(row, { clientX: 120, clientY: 80 })
    const menu = screen.getByRole('menu', { name: 'Vendor audit actions' })
    expect(menu).toHaveStyle({ left: '120px', top: '80px' })
    const remove = within(menu).getByRole('menuitem', { name: 'Delete Vendor audit' })
    expect(within(menu).getByRole('menuitem', { name: 'Rename' })).toHaveFocus()

    await fireEvent.keyDown(menu, { key: 'Escape' })
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
    await waitFor(() => expect(row).toHaveFocus())

    // The keyboard's context menu key opens it under the row.
    await fireEvent.contextMenu(row)
    expect(screen.getByRole('menu', { name: 'Vendor audit actions' })).toHaveStyle({ left: '8px', top: '8px' })
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete Vendor audit' }))
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
    expect(within(screen.getByLabelText('Delete Vendor audit?')).getAllByRole('button').map((button) => button.textContent)).toEqual(['Delete', 'Cancel'])

    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByLabelText('Delete Vendor audit?')).not.toBeInTheDocument()
    await waitFor(() => expect(row).toHaveFocus())

    await openDeleteMenu('Vendor audit')
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Delete' }), { key: 'Escape' })
    expect(screen.queryByLabelText('Delete Vendor audit?')).not.toBeInTheDocument()
    await waitFor(() => expect(row).toHaveFocus())

    // The current thread's row opens the same menu.
    await openDeleteMenu('Lease renewal')
    expect(screen.getByLabelText('Delete Lease renewal?')).toBeInTheDocument()
  })

  it('selects rows with Shift and Command clicks and deletes the selection in one confirm', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' },
      { threadId: 'thread-2', title: 'Vendor audit', updatedAt: '' },
      { threadId: 'thread-3', title: 'Archive review', updatedAt: '' },
      { threadId: 'thread-4', title: 'Budget notes', updatedAt: '' },
    ]
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'chat_delete_thread') return undefined
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const vendor = await screen.findByRole('button', { name: 'Vendor audit' })
    const archive = screen.getByRole('button', { name: 'Archive review' })
    await fireEvent.click(vendor, { metaKey: true })
    await fireEvent.click(archive, { shiftKey: true })
    expect(document.querySelectorAll('.thread-row.selected')).toHaveLength(2)

    await fireEvent.contextMenu(archive)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete 2' }))
    // Several rows confirm with two controls and no sentence.
    const confirm = screen.getByLabelText('Delete 2 threads?')
    expect(within(confirm).getAllByRole('button').map((button) => button.textContent)).toEqual(['Delete 2', 'Cancel'])
    await fireEvent.click(within(confirm).getByRole('button', { name: 'Delete 2' }))
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'chat_delete_thread')).toHaveLength(2))
    expect(invoke).toHaveBeenCalledWith('chat_delete_thread', { threadId: 'thread-2' })
    expect(invoke).toHaveBeenCalledWith('chat_delete_thread', { threadId: 'thread-3' })
    expect(document.querySelectorAll('.thread-row.selected')).toHaveLength(0)

    // Delete on a focused row opens the confirm for that row alone.
    const budget = screen.getByRole('button', { name: 'Budget notes' })
    await fireEvent.keyDown(budget, { key: 'Delete' })
    expect(within(screen.getByLabelText('Delete Budget notes?')).getAllByRole('button').map((button) => button.textContent)).toEqual(['Delete', 'Cancel'])
  })

  it('deletes a selection that holds the open thread and lands on a fresh thread', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' },
      { threadId: 'thread-2', title: 'Vendor audit', updatedAt: '' },
      { threadId: 'thread-3', title: 'Archive review', updatedAt: '' },
    ]
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{ runId: 'run-1', phase: 'complete', prompt: 'Question', text: 'Answer', toolActivity: [] }]
      if (command === 'chat_delete_thread') return undefined
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByText('Answer')
    const current = document.querySelector('.thread-row[aria-current="true"]')
    expect(current).toHaveTextContent('Lease renewal')
    await fireEvent.click(current, { metaKey: true })
    await fireEvent.click(screen.getByRole('button', { name: 'Vendor audit' }), { metaKey: true })
    expect(document.querySelectorAll('.thread-row.selected')).toHaveLength(2)

    // A right-click on a row outside the selection makes that row the selection.
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'Archive review' }))
    expect(screen.getByRole('menuitem', { name: 'Delete Archive review' })).toBeInTheDocument()
    expect(document.querySelectorAll('.thread-row.selected')).toHaveLength(0)
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })

    await fireEvent.click(current, { metaKey: true })
    await fireEvent.click(screen.getByRole('button', { name: 'Vendor audit' }), { metaKey: true })
    await fireEvent.contextMenu(current)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete 2' }))
    await fireEvent.click(within(screen.getByLabelText('Delete 2 threads?')).getByRole('button', { name: 'Delete 2' }))
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'chat_delete_thread')).toHaveLength(2))
    await waitFor(() => expect(screen.queryByText('Answer')).not.toBeInTheDocument())
    expect(document.querySelector('[data-fresh-thread]')).toBeInTheDocument()
    expect(within(screen.getByRole('list', { name: 'Threads' })).getAllByRole('listitem')).toHaveLength(2)
    expect(screen.getByPlaceholderText('Ask anything')).toHaveFocus()
  })

  it('closes the thread menu on a pointer press outside it', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    render(App)
    await screen.findByText('Lease renewal', { selector: '.thread-row-title' })
    await fireEvent.contextMenu(document.querySelector('.thread-record .thread-row'), { clientX: 120, clientY: 80 })
    expect(screen.getByRole('menu', { name: 'Lease renewal actions' })).toBeInTheDocument()
    await fireEvent.pointerDown(screen.getByPlaceholderText('Ask anything'), { button: 0 })
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })

  it('deletes the open thread and focuses the fresh composer', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{ runId: 'run-1', phase: 'complete', prompt: 'Question', text: 'Answer', toolActivity: [] }]
      if (command === 'chat_delete_thread') return undefined
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByText('Answer')

    await openDeleteMenu('Lease renewal')
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }))

    await waitFor(() => expect(screen.queryByText('Answer')).not.toBeInTheDocument())
    expect(invoke).toHaveBeenCalledWith('chat_delete_thread', { threadId: 'thread-1' })
    expect(document.querySelector('[data-fresh-thread]')).toBeInTheDocument()
    expect(within(screen.getByRole('list', { name: 'Threads' })).getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getByPlaceholderText('Ask anything')).toHaveFocus()
  })

  it('keeps the row and reports a failed delete', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    let deleteAttempts = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'chat_delete_thread' && deleteAttempts++ === 0) throw new Error('offline')
      if (command === 'chat_delete_thread') return undefined
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByText('Lease renewal', { selector: '.thread-row-title' })
    await openDeleteMenu('Lease renewal')
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('The thread could not be deleted.')
    const repeatDelete = screen.getByRole('button', { name: 'Delete thread' })
    expect(screen.getByText('Lease renewal', { selector: '.thread-row-title' })).toBeInTheDocument()
    expect(screen.queryByLabelText('Delete Lease renewal?')).not.toBeInTheDocument()

    await fireEvent.click(repeatDelete)

    await waitFor(() => expect(screen.queryByText('The thread could not be deleted.')).not.toBeInTheDocument())
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_delete_thread')).toHaveLength(2)
  })

  it('uses the stored title and renames it from the keyboard', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Stored name', updatedAt: '' }]
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-1', phase: 'complete', prompt: 'Derived name', text: 'Answer', toolActivity: [],
      }]
      if (command === 'chat_rename_thread') return undefined
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    await screen.findByText('Answer')
    const name = screen.getByRole('button', { name: 'Rename thread' })
    name.focus()
    await fireEvent.keyDown(name, { key: 'Enter' })
    const field = await screen.findByRole('textbox', { name: 'Thread name' })
    expect(field).toHaveValue('Stored name')
    expect(field).toHaveAttribute('maxlength', '160')
    expect(field.selectionStart).toBe(0)
    expect(field.selectionEnd).toBe('Stored name'.length)

    await fireEvent.input(field, { target: { value: '  Renamed thread  ' } })
    await fireEvent.keyDown(field, { key: 'Enter' })

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_rename_thread', {
      threadId: 'thread-1',
      title: 'Renamed thread',
    }))
    const renamed = screen.getByRole('button', { name: 'Rename thread' })
    expect(renamed).toHaveTextContent('Renamed thread')
    expect(renamed).toHaveFocus()
    expect(document.querySelector('.thread-row[aria-current="true"]')).toHaveTextContent('Renamed thread')
  })

  it('caps a thread name at 80 Unicode scalars', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Stored name', updatedAt: '' }]
    render(App)
    const name = await screen.findByRole('button', { name: 'Rename thread' })

    await fireEvent.click(name)
    const field = await screen.findByRole('textbox', { name: 'Thread name' })
    await fireEvent.input(field, { target: { value: `${'😀'.repeat(80)}x` } })

    expect(field).toHaveValue('😀'.repeat(80))
  })

  it('commits on blur and restores focus to the rename button', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Stored name', updatedAt: '' }]
    render(App)
    const name = await screen.findByRole('button', { name: 'Rename thread' })

    await fireEvent.click(name)
    const field = await screen.findByRole('textbox', { name: 'Thread name' })
    await fireEvent.input(field, { target: { value: 'Blurred name' } })
    await fireEvent.blur(field)

    const renamed = screen.getByRole('button', { name: 'Rename thread' })
    await waitFor(() => expect(renamed).toHaveFocus())
    expect(invoke).toHaveBeenCalledWith('chat_rename_thread', {
      threadId: 'thread-1',
      title: 'Blurred name',
    })
  })

  it('cancels a rename with Escape', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Stored name', updatedAt: '' }]
    render(App)
    await waitFor(() => expect(screen.getByRole('button', { name: 'Rename thread' })).toBeEnabled())
    const name = screen.getByRole('button', { name: 'Rename thread' })

    await fireEvent.keyDown(name, { key: 'Enter' })
    const field = await screen.findByRole('textbox', { name: 'Thread name' })
    await fireEvent.input(field, { target: { value: 'Discarded name' } })
    await fireEvent.keyDown(field, { key: 'Escape' })

    const restored = screen.getByRole('button', { name: 'Rename thread' })
    expect(restored).toHaveTextContent('Stored name')
    expect(restored).toHaveFocus()
    expect(invoke).not.toHaveBeenCalledWith('chat_rename_thread', expect.anything())
  })

  it('renders every summary and opens another thread from its button', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Lease renewal', updatedAt: '2026-07-28T11:55:00Z' },
      { threadId: 'thread-2', title: 'Archive review', updatedAt: '2026-07-28T09:00:00Z' },
      { threadId: 'thread-3', title: 'Client notes', updatedAt: '2026-07-25T12:00:00Z' },
    ]
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_select_thread') return undefined
      if (command === 'chat_thread_open') return payload.threadId === 'thread-2'
        ? [{ runId: 'run-2', phase: 'complete', prompt: 'Archive review', text: 'Archived.', toolActivity: [] }]
        : []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    const current = await screen.findByText('Lease renewal')
    expect(current.closest('.thread-row')).toHaveAttribute('aria-current', 'true')
    expect(document.querySelectorAll('.thread-row')).toHaveLength(3)
    for (const summary of threadSummaryResult) {
      expect(document.querySelector(`time[datetime="${summary.updatedAt}"]`)).not.toHaveAttribute('title')
    }
    const archive = screen.getByRole('button', { name: /^Archive review/ })
    expect(archive.tabIndex).toBe(0)

    await fireEvent.click(archive)

    expect(await screen.findByText('Archived.')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('chat_select_thread', { threadId: 'thread-2' })
    expect(archive).not.toBeInTheDocument()
    expect(document.querySelector('.thread-row[aria-current="true"]')).toHaveTextContent('Archive review')
  })

  it('shows the empty name in the titlebar and current thread record', async () => {
    render(App)
    await findWorkspaceComposer()

    const titlebarName = document.querySelector('.thread-title')
    const sidebarName = document.querySelector('.thread-row')
    expect(titlebarName).toHaveTextContent('New thread')
    expect(titlebarName).not.toHaveAttribute('title')
    expect(sidebarName).toHaveTextContent('New thread')
    expect(sidebarName).not.toHaveAttribute('title')
    expect(screen.queryByText('local · durable')).not.toBeInTheDocument()
  })

  it('shows the first restored prompt in the titlebar and current thread record', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-1',
        phase: 'complete',
        text: 'The renewal date is September 1.',
        prompt: '  Review \n the lease renewal  ',
        receipt: null,
        toolActivity: [],
      }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByText('The renewal date is September 1.')

    const titlebarName = document.querySelector('.thread-title')
    const sidebarName = document.querySelector('.thread-row')
    expect(titlebarName).toHaveTextContent('Review the lease renewal')
    expect(titlebarName).not.toHaveAttribute('title')
    expect(sidebarName).toHaveTextContent('Review the lease renewal')
    expect(sidebarName).not.toHaveAttribute('title')
    expect(screen.queryByText('local · durable')).not.toBeInTheDocument()
  })

  it('uses one-line ellipsis styles for both thread names', () => {
    expect(rowControlRules.get('.thread-title')).toMatch(/overflow:\s*hidden/)
    expect(rowControlRules.get('.thread-title')).toMatch(/text-overflow:\s*ellipsis/)
    expect(rowControlRules.get('.thread-title')).toMatch(/white-space:\s*nowrap/)
    expect(appRules.get('input.thread-title')).toMatch(/text-overflow:\s*ellipsis/)
    expect(appRules.get('.thread-row-title')).toMatch(/overflow:\s*hidden/)
    expect(appRules.get('.thread-row-title')).toMatch(/white-space:\s*nowrap/)
    // The row title fades at its end and the time keeps its width, so the time always shows.
    expect(appRules.get('.thread-row-title')).toMatch(/flex:\s*1 1 auto/)
    expect(appRules.get('.thread-row-title')).toMatch(/mask-image:\s*linear-gradient\(to right, currentColor calc\(100% - 28px\), transparent\)/)
    expect(appRules.get('.thread-row time')).toMatch(/flex:\s*none/)
  })

  it('reserves the current-thread dot width in every row', () => {
    expect(appRules.get('.thread-row > span')).toMatch(/flex:\s*0 0 5px/)
  })
})

describe('window chrome', () => {
  it.each(['MacIntel', 'Win32', 'Linux x86_64'])('keeps the row controls on %s', async (platform) => {
    const platformMock = vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform)
    try {
      render(App)
      const collapse = await screen.findByRole('button', { name: 'Collapse sidebar' })
      const row = document.querySelector('.titlebar')
      expect(row).toHaveAttribute('data-tauri-drag-region')
      expect(row.closest('.workspace').classList.contains('macos')).toBe(platform.startsWith('Mac'))
      expect([...row.querySelectorAll('button')].map((button) => button.getAttribute('aria-label'))).toEqual([
        'Collapse sidebar', 'Rename thread', 'Thread actions', 'Open artifact rail', 'Open record panel',
      ])
      expect(row.querySelector('.title-spacer')).toHaveAttribute('data-tauri-drag-region')
      expect(row.querySelector('.update-slot')).toBeEmptyDOMElement()
      expect(row.querySelector('.artifacts-toggle kbd')).toHaveTextContent(platform.startsWith('Mac') ? '⌘ J' : 'Ctrl J')
      expect(row.querySelector('.record-toggle kbd')).toHaveTextContent(platform.startsWith('Mac') ? '⌘ K' : 'Ctrl K')
      for (const control of row.querySelectorAll('button, input, button *')) {
        expect(control).not.toHaveAttribute('data-tauri-drag-region')
      }
      await fireEvent.click(collapse)
      expect(row).toContainElement(screen.getByRole('button', { name: 'Expand sidebar' }))
      expect(screen.queryByRole('button', { name: 'New thread' })).not.toBeInTheDocument()
      await fireEvent.click(screen.getByRole('button', { name: 'Open artifact rail' }))
      expect(row).toContainElement(screen.getByRole('button', { name: 'Close artifact rail' }))
    } finally {
      cleanup()
      platformMock.mockRestore()
    }
  })

  it('uses the title actions with a collapsed sidebar and keeps delete confirmation visible', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    render(App)
    const actions = await screen.findByRole('button', { name: 'Thread actions' })
    await waitFor(() => expect(actions).toBeEnabled())
    await fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }))
    await fireEvent.click(actions)
    expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual(['Rename', 'Pin', 'Archive', 'Delete'])
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Pin' }))
    await fireEvent.click(actions)
    expect(screen.getByRole('menuitem', { name: 'Unpin' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Archive' }))
    await fireEvent.click(actions)
    expect(screen.getByRole('menuitem', { name: 'Restore' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const input = screen.getByRole('textbox', { name: 'Thread name' })
    expect(input).toHaveValue('Lease renewal')
    await fireEvent.keyDown(input, { key: 'Escape' })
    await fireEvent.click(actions)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete Lease renewal' }))
    const confirmation = screen.getByRole('group', { name: 'Delete Lease renewal?' })
    expect(document.querySelector('.titlebar')).toContainElement(confirmation)
    await fireEvent.click(within(confirmation).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(actions).toHaveFocus())
    expect(screen.queryByRole('group', { name: 'Delete Lease renewal?' })).not.toBeInTheDocument()
  })

  it('shows no tooltip on any element of the shell', () => {
    const files = fs.readdirSync('src', { recursive: true }).filter((file) => file.endsWith('.svelte'))
    expect(files.length).toBeGreaterThan(5)
    for (const file of files) {
      // A lowercase tag is an HTML element, and a title attribute there is a hover tooltip. A component prop named title is a heading.
      expect(fs.readFileSync(`src/${file}`, 'utf8'), file).not.toMatch(/<[a-z][^>]*\stitle=/)
    }
    expect(fs.readFileSync('src/lib/assistant-markdown.js', 'utf8')).not.toMatch(/title=/)
  })

  it('centers the title row controls and the traffic lights on the 36px band above the panels', () => {
    const config = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'))
    const main = config.app.windows.find((window) => window.label === 'main')
    expect(appStyles).toMatch(/grid-template-rows:\s*var\(--titlebar-height\) minmax\(0, 1fr\)/)
    expect(appStyles).toMatch(/row-gap:\s*calc\(var\(--titlebar-band\) - var\(--titlebar-height\)\);\s*column-gap:\s*var\(--frame-width\)/)
    const band = Number(appStyles.match(/--titlebar-band:\s*(\d+)px/)[1])
    const rowHeight = Number(appRules.get('.workspace.macos').match(/--titlebar-height:\s*(\d+)px/)[1])
    const inset = Number(appRules.get('.workspace.macos').match(/--titlebar-inset:\s*(\d+)px/)[1])
    const controlHeight = Number(appRules.get('.titlebar button, .titlebar input').match(/(?:^|;)\s*height:\s*(\d+)px/)[1])
    // The band runs from the top edge to the panels, so the controls at the end of the row and the 14pt lights both center on it.
    // Measured on macOS 26: AppKit tops the 14pt lights at 9pt from x = 9, and the row's default matches the frame gap.
    expect(band).toBe(36)
    expect(rowHeight).toBe(30)
    expect(Number(appStyles.match(/--frame-width: 8px; --titlebar-band: \d+px; --titlebar-height:\s*(\d+)px/)[1])).toBe(rowHeight)
    expect(controlHeight).toBe(24)
    expect(rowHeight - controlHeight / 2).toBe(band / 2)
    expect(main.trafficLightPosition).toEqual({ x: 9, y: band / 2 - 14 / 2 })
    for (const selector of ['.titlebar', '.workspace.macos .titlebar-sidebar', '.workspace.macos .titlebar-thread']) {
      expect(appRules.get(selector), selector).toMatch(/align-items:\s*end/)
    }
    // The clearance is the sidebar part's padding: the title row itself is a subgrid with no padding.
    expect(appRules.get('.workspace.macos .titlebar')).toMatch(/grid-template-columns:\s*subgrid;\s*margin:\s*0;\s*padding:\s*0/)
    expect(appRules.get('.workspace.macos .titlebar-sidebar')).toMatch(/padding-left:\s*calc\(var\(--titlebar-inset\) - var\(--frame-width\)\)/)
    expect(main.visible).toBe(false)
    // Three 14pt lights from x=9 with 9pt gaps end at 69pt, and the row starts one gap later.
    expect(inset).toBeGreaterThanOrEqual(9 + 3 * 14 + 2 * 9 + 9)
    expect(inset).toBe(78)
  })

  it('keeps native decorations and grants row drag permission', () => {
    const config = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'))
    const capabilities = JSON.parse(fs.readFileSync('src-tauri/capabilities/default.json', 'utf8'))
    expect(config.app.windows[0]).toMatchObject({
      title: 'muniment', titleBarStyle: 'Overlay', hiddenTitle: true,
    })
    expect(config.app.windows[0].decorations).not.toBe(false)
    expect(capabilities.permissions).toContain('core:window:allow-start-dragging')
  })
})

describe('sidebar collapse', () => {
  it('keeps saved agents outside project thread lists', async () => {
    const original = invoke.getMockImplementation()
    invoke.mockImplementation(async (command, args) => command === 'agent_list'
      ? { agents: [{ id: 'agent-1', name: 'Scout', instructions: 'Read sources.', projectId: null, schedule: null }], state: { runs: {}, threads: {} } }
      : original(command, args))
    render(App)
    const manager = await screen.findByRole('button', { name: 'Agents', exact: true })
    await fireEvent.click(manager)
    const dialog = await screen.findByRole('region', { name: 'Agents', exact: true })
    await within(dialog).findByRole('button', { name: /Scout/ })
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Close agents' }))
    const scout = screen.getByRole('button', { name: 'Scout', exact: true })
    expect(scout.closest('.agent-roster')).not.toBeNull()
    expect(scout.closest('.project-section')).toBeNull()
    expect(scout.closest('.thread-list')).toBeNull()
  })

  it('opens the existing agent chat with one shared composer and restores a regular chat', async () => {
    const original = invoke.getMockImplementation()
    invoke.mockImplementation(async (command, args) => command === 'agent_list'
      ? { agents: [{ id: 'agent-1', name: 'Scout', instructions: 'Read sources.', projectId: null, schedule: null }], state: { runs: {}, threads: { 'agent-thread': 'agent-1' } } }
      : command === 'chat_thread_summaries' ? { summaries: [{ threadId: 'agent-thread', title: 'Hidden agent transcript', updatedAt: '2026-01-01T00:00:00Z' }, { threadId: 'ordinary-thread', title: 'Ordinary chat', updatedAt: '2026-01-01T00:00:00Z' }], nextCursor: null }
      : ['chat_select_thread', 'chat_new_thread'].includes(command) ? undefined : original(command, args))
    render(App)
    const agents = await screen.findByRole('button', { name: 'Agents', exact: true })
    await fireEvent.click(agents)
    const catalog = await screen.findByRole('region', { name: 'Agents', exact: true })
    await fireEvent.click(await within(catalog).findByRole('button', { name: /Scout/ }))
    await screen.findByRole('complementary', { name: 'Agent profile' })
    expect(document.querySelector('.sidebar')?.textContent || document.querySelector('#sidebar').textContent).not.toContain('Hidden agent transcript')
    expect(screen.queryByRole('button', { name: 'Rename thread' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Thread actions' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Scout', exact: true })).toHaveAttribute('aria-current', 'true')
    expect(invoke).toHaveBeenCalledWith('chat_select_thread', { threadId: 'agent-thread' })
    expect(invoke).toHaveBeenCalledWith('chat_thread_open', { threadId: 'agent-thread', limit: 100 })
    expect(invoke.mock.calls.filter(([cmd]) => cmd === 'chat_new_thread')).toHaveLength(0)
    expect(screen.getAllByRole('textbox', { name: 'Message' })).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'Add files' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Voice' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Close agent profile' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Scout', exact: true }))
    await screen.findByRole('complementary', { name: 'Agent profile' })
    expect(invoke.mock.calls.filter(([cmd]) => cmd === 'chat_new_thread')).toHaveLength(0)
    await fireEvent.click(document.querySelector('.new-thread'))
    await waitFor(() => expect(screen.queryByRole('complementary', { name: 'Agent profile' })).not.toBeInTheDocument())
    expect(invoke).toHaveBeenCalledWith('chat_new_thread')
    expect(screen.getAllByRole('textbox', { name: 'Message' })).toHaveLength(1)
  })

  it('creates the first agent chat once and keeps its project assignment', async () => {
    const original = invoke.getMockImplementation()
    const listing = { agents: [{ id: 'agent-1', name: 'Scout', instructions: 'Read sources.', projectId: 'project-1', schedule: null }], state: { runs: {}, threads: {} } }
    invoke.mockImplementation(async (command, args) => {
      if (command === 'agent_list') return listing
      if (command === 'chat_new_thread') { listing.state.threads['agent-thread'] = 'agent-1'; return 'agent-thread' }
      if (command === 'project_list') return { projects: { 'project-1': 'Research' }, threads: { 'agent-thread': 'project-1' } }
      return original(command, args)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Scout', exact: true }))
    await screen.findByRole('complementary', { name: 'Agent profile' })
    expect(invoke).toHaveBeenCalledWith('chat_new_thread', { agentId: 'agent-1', projectId: 'project-1' })
    expect(document.querySelector('[data-fresh-thread]')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: 'Close agent profile' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Scout', exact: true }))
    await screen.findByRole('complementary', { name: 'Agent profile' })
    expect(invoke.mock.calls.filter(([cmd]) => cmd === 'chat_new_thread')).toHaveLength(1)
    expect(screen.getAllByRole('textbox', { name: 'Message' })).toHaveLength(1)
  })

  const sidebarShortcut = () => navigator.platform.startsWith('Mac')
    ? { key: '\\', metaKey: true }
    : { key: '\\', ctrlKey: true }

  it('collapses to nothing from the app row control and expands again', async () => {
    render(App)
    const collapse = await screen.findByRole('button', { name: 'Collapse sidebar' })
    expect(collapse).toHaveAttribute('aria-expanded', 'true')
    expect(collapse).toHaveAttribute('aria-controls', 'sidebar')
    expect(collapse).toHaveAttribute('aria-keyshortcuts', navigator.platform.startsWith('Mac') ? 'Meta+\\' : 'Control+\\')
    expect(screen.getByRole('list', { name: 'Threads' })).toBeInTheDocument()
    const newThread = document.querySelector('.new-thread')
    expect(newThread).toHaveAttribute('aria-keyshortcuts', navigator.platform.startsWith('Mac') ? 'Meta+N' : 'Control+N')
    expect(newThread.querySelector('kbd')).toHaveTextContent(navigator.platform.startsWith('Mac') ? '⌘ N' : 'Ctrl N')
    const currentThread = document.querySelector('.thread-row')
    expect(currentThread).toHaveTextContent('New thread')
    expect(currentThread).toHaveAttribute('aria-current', 'true')
    expect(currentThread).not.toHaveAttribute('tabindex')
    expect(currentThread.tabIndex).toBe(-1)
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument()
    expect(document.querySelectorAll('.titlebar .new-thread, #sidebar .side-action')).toHaveLength(4)
    expect(screen.getByRole('button', { name: 'Settings' })).toHaveTextContent('Settings')
    expect(document.querySelectorAll('.titlebar .new-thread kbd')).toHaveLength(0)
    expect(document.querySelectorAll('#sidebar kbd')).toHaveLength(1)

    collapse.focus()
    await fireEvent.click(collapse)

    const expand = screen.getByRole('button', { name: 'Expand sidebar' })
    // The toggle is one persistent element, so keyboard focus survives the toggle.
    expect(expand).toBe(collapse)
    expect(document.activeElement).toBe(expand)
    expect(expand).toHaveAttribute('aria-expanded', 'false')
    expect(expand).not.toHaveAttribute('title')
    expect(screen.queryByRole('button', { name: 'New thread' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument()
    // Collapsed means gone: no rail, and Settings hides with the sidebar.
    expect(screen.queryByRole('button', { name: 'Settings' })).not.toBeInTheDocument()
    expect(document.getElementById('sidebar').textContent.trim()).toBe('')
    expect(within(document.getElementById('sidebar')).queryAllByRole('button')).toHaveLength(0)
    expect(screen.queryByText('Threads')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Alice/i })).not.toBeInTheDocument()

    await fireEvent.click(expand)

    expect(screen.getByRole('button', { name: 'Collapse sidebar' })).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('list', { name: 'Threads' })).toBeInTheDocument()
    expect(await screen.findByRole('button', { name: /Alice/i })).toBeInTheDocument()
  })

  it('toggles with the platform keyboard shortcut from editable fields', async () => {
    render(App)
    const collapse = await screen.findByRole('button', { name: 'Collapse sidebar' })

    await fireEvent.keyDown(document, sidebarShortcut())
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
    await fireEvent.keyDown(document, sidebarShortcut())
    expect(screen.getByRole('button', { name: 'Collapse sidebar' })).toBeInTheDocument()

    await fireEvent.keyDown(screen.getByPlaceholderText('Ask anything'), sidebarShortcut())
    expect(collapse).toHaveAttribute('aria-expanded', 'false')

    const input = document.createElement('input')
    document.body.append(input)
    await fireEvent.keyDown(input, sidebarShortcut())
    expect(screen.getByRole('button', { name: 'Collapse sidebar' })).toBeInTheDocument()
    input.remove()
  })

  it('ignores the sidebar shortcut outside the signed-in workspace', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    render(App)
    await screen.findByRole('textbox', { name: 'Message' })
    const event = new KeyboardEvent('keydown', { ...sidebarShortcut(), cancelable: true })

    document.dispatchEvent(event)

    expect(event.defaultPrevented).toBe(false)
    expect(localStorage.getItem('muniment.sidebar-collapsed')).toBeNull()
  })

  it('remembers the collapsed choice across a reload and defaults to expanded', async () => {
    const first = render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Collapse sidebar' }))
    expect(localStorage.getItem('muniment.sidebar-collapsed')).toBe('collapsed')
    first.unmount()

    const reloaded = render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Expand sidebar' }))
    expect(localStorage.getItem('muniment.sidebar-collapsed')).toBe('expanded')
    reloaded.unmount()

    localStorage.setItem('muniment.sidebar-collapsed', 'not a state we ever wrote')
    render(App)
    expect(await screen.findByRole('button', { name: 'Collapse sidebar' })).toBeInTheDocument()
  })

  it('keeps working when reading and writing the remembered state throws', async () => {
    const getItem = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new DOMException('private storage detail', 'SecurityError') })
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new DOMException('private storage detail', 'QuotaExceededError') })

    render(App)
    const collapse = await screen.findByRole('button', { name: 'Collapse sidebar' })
    await fireEvent.click(collapse)

    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument()
    expect(screen.queryByText(/private storage detail/)).not.toBeInTheDocument()
    getItem.mockRestore()
    setItem.mockRestore()
  })

  it('widens the artifact rail bounds while the sidebar is a rail', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Open artifact rail' }))
    expect(screen.getByRole('separator', { name: 'Artifacts' })).toHaveAttribute('aria-valuemax', '509')

    await fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }))
    const separator = screen.getByRole('separator', { name: 'Artifacts' })
    expect(separator).toHaveAttribute('aria-valuemax', '560')

    await fireEvent.keyDown(separator, { key: 'End' })
    expect(separator).toHaveAttribute('aria-valuenow', '560')
    await fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }))
    expect(screen.getByRole('separator', { name: 'Artifacts' })).toHaveAttribute('aria-valuenow', '509')
  })
})

describe('thread row shortcuts', () => {
  const rowShortcut = (position) => ({
    key: position.toString(),
    code: `Digit${position}`,
    metaKey: navigator.platform.startsWith('Mac'),
    ctrlKey: !navigator.platform.startsWith('Mac'),
  })

  it('opens a rendered row from the focused composer and names the first nine rows', async () => {
    threadSummaryResult = Array.from({ length: 10 }, (_, index) => ({
      threadId: `thread-${index + 1}`,
      title: `Thread ${index + 1}`,
      updatedAt: '2026-01-01T00:00:00Z',
    }))
    render(App)
    const composer = await findWorkspaceComposer()
    await waitFor(() => expect(document.querySelectorAll('.thread-row')).toHaveLength(10))
    invoke.mockClear()

    const rows = document.querySelectorAll('.thread-row')
    const modifier = navigator.platform.startsWith('Mac') ? 'Meta' : 'Control'
    for (let index = 0; index < 9; index += 1) {
      expect(rows[index]).toHaveAttribute('aria-keyshortcuts', `${modifier}+${index + 1}`)
    }
    expect(rows[9]).not.toHaveAttribute('aria-keyshortcuts')

    composer.focus()
    const event = new KeyboardEvent('keydown', { ...rowShortcut(2), bubbles: true, cancelable: true })
    composer.dispatchEvent(event)

    expect(event.defaultPrevented).toBe(true)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_select_thread', { threadId: 'thread-2' }))
  })

  it('does not reopen the current row or open a position past the rendered rows', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Current', updatedAt: '' },
      { threadId: 'thread-2', title: 'Other', updatedAt: '' },
    ]
    render(App)
    await findWorkspaceComposer()
    invoke.mockClear()

    await fireEvent.keyDown(document, rowShortcut(1))
    await fireEvent.keyDown(document, rowShortcut(9))

    expect(invoke).not.toHaveBeenCalledWith('chat_thread_open', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_select_thread', expect.anything())
  })
})

describe('new thread', () => {
  const newThreadShortcut = () => navigator.platform.startsWith('Mac')
    ? { key: 'n', metaKey: true }
    : { key: 'n', ctrlKey: true }

  it('clears the transcript, shows a fresh row, and focuses the composer', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-1', phase: 'complete', text: 'Current answer',
        prompt: 'Current question', receipt: null, toolActivity: [],
      }]
      if (command === 'chat_new_thread') return null
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByText('Current answer')

    await fireEvent.click(screen.getByRole('button', { name: 'New thread' }))

    await waitFor(() => expect(screen.queryByText('Current answer')).not.toBeInTheDocument())
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_new_thread')).toHaveLength(1)
    const fresh = document.querySelector('[data-fresh-thread]')
    expect(fresh).toHaveTextContent('New thread')
    expect(fresh).toHaveAttribute('aria-current', 'true')
    expect(fresh).toHaveAttribute('aria-keyshortcuts', navigator.platform.startsWith('Mac') ? 'Meta+1' : 'Control+1')
    expect(fresh.tagName).toBe('LI')
    expect(document.querySelectorAll('.thread-row[aria-current="true"]')).toHaveLength(1)
    expect(document.querySelectorAll('.thread-list button[aria-current="true"]')).toHaveLength(0)
    expect(document.activeElement).toBe(screen.getByPlaceholderText('Ask anything'))
  })

  it('starts from the focused composer with the platform chord', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'chat_new_thread') return null
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await waitFor(() => expect(document.querySelector('.thread-row[aria-current="true"]')).toBeInTheDocument())
    composer.focus()

    await fireEvent.keyDown(composer, newThreadShortcut())

    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'chat_new_thread')).toHaveLength(1))
    expect(document.activeElement).toBe(composer)
  })

  it('keeps the transcript and current row when the command fails', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-1', phase: 'complete', text: 'Current answer',
        prompt: 'Current question', receipt: null, toolActivity: [],
      }]
      if (command === 'chat_new_thread') throw new Error('offline')
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByText('Current answer')

    await fireEvent.click(screen.getByRole('button', { name: 'New thread' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('A new thread could not be started.')
    expect(screen.getByText('Current answer')).toBeInTheDocument()
    expect(document.querySelector('.thread-row[aria-current="true"]')).toHaveTextContent('Current question')
  })

  it('keeps the fresh row current until the submitted thread appears', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'chat_new_thread') return null
      if (command === 'chat_submit') return { runId: 'run-new', attachments: [] }
      if (command === 'chat_current_thread') return 'thread-new'
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    const newThread = screen.getByRole('button', { name: 'New thread' })
    await waitFor(() => expect(newThread).toBeEnabled())
    await fireEvent.click(newThread)
    await waitFor(() => expect(document.querySelector('[data-fresh-thread]')).toBeInTheDocument())
    await fireEvent.input(composer, { target: { value: 'First prompt' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    expect(document.querySelector('[data-fresh-thread]')).toHaveAttribute('aria-current', 'true')
    const threadOpenCount = invoke.mock.calls.filter(([command]) => command === 'chat_thread_open').length
    threadSummaryResult = [{ threadId: 'thread-new', title: 'First prompt', updatedAt: '' }]
    chatListener({ payload: { runId: 'run-new', type: 'completed', receipt: null } })

    await waitFor(() => expect(document.querySelector('[data-fresh-thread]')).not.toBeInTheDocument())
    expect(document.querySelector('.thread-row[aria-current="true"]')).toHaveTextContent('First prompt')
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_thread_open')).toHaveLength(threadOpenCount)
  })
})

describe('Home onboarding', () => {
  beforeEach(() => {
    const original = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => {
      if (command === 'attach_listener_status') return Promise.resolve({ connected: true, supervisor_running: true })
      return original(command, payload)
    })
  })

  function firstRun() {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
  }

  async function firstSend(text = 'Keep this draft') {
    const composer = await screen.findByRole('textbox', { name: 'Message' })
    await fireEvent.input(composer, { target: { value: text } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
  }

  function finding(assistantId, displayName, fileCount, overrides = {}) {
    return { assistantId, displayName, fileCount, root: '/assistant', byteTotal: 0, capped: false, warnings: [], ...overrides }
  }

  it('shows the composer above three mono chips without launch writes or questions', async () => {
    firstRun()
    render(App)
    const composer = await screen.findByRole('textbox', { name: 'Message' })
    expect(composer).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled', 'true')
    await waitFor(() => expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment'))
    const chips = screen.getByLabelText('First-run settings')
    expect(within(chips).getAllByRole('button')).toHaveLength(3)
    expect(composer.compareDocumentPosition(chips) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(screen.getByTestId('onboarding-model')).toHaveTextContent('Connect a model')
    expect(await screen.findByText('No assistant memory found')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('onboarding_scan')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    expect(invoke.mock.calls.some(([command]) => command.startsWith('onboarding_import'))).toBe(false)
    expect(screen.queryByTestId('onboarding-confirm')).not.toBeInTheDocument()
    expect(screen.queryByText('Continue without importing')).not.toBeInTheDocument()
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument()
    expect(screen.queryByText('Sign in')).not.toBeInTheDocument()
    const source = fs.readFileSync('src/lib/Onboarding.svelte', 'utf8')
    expect(source).toMatch(/\.chips button\s*\{[^}]*font:[^}]*var\(--font-mono\)/)
  })

  it('keeps the composer and draft while Home loads and retries an early Send without launch writes', async () => {
    const status = deferred()
    homeStatus = status.promise
    render(App)
    const composer = screen.getByRole('textbox', { name: 'Message' })
    const send = screen.getByRole('button', { name: 'Send' })
    const chips = screen.getByLabelText('First-run settings')
    expect(composer).toBeEnabled()
    expect(send).toBeEnabled()
    expect(send).not.toHaveAttribute('aria-disabled', 'true')
    expect(within(chips).getAllByRole('button')).toHaveLength(3)
    expect(composer.compareDocumentPosition(chips) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('Finding Home…')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())

    await fireEvent.input(composer, { target: { value: 'My early draft' } })
    await fireEvent.click(send)
    expect(screen.getByRole('alert')).toHaveTextContent('Home is unavailable. Retry or choose a folder.')
    expect(screen.getByRole('textbox', { name: 'Message' })).toBe(composer)
    expect(composer).toHaveValue('My early draft')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())

    status.resolve({ configured: false, homePath: '/Documents/Muniment' })
    await waitFor(() => expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment'))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Message' })).toBe(composer)
    expect(composer).toHaveValue('My early draft')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await fireEvent.click(send)
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    expect(await screen.findByRole('button', { name: 'Open model settings' })).toBeEnabled()
  })

  it('preserves an early draft when Home resolves to a configured path', async () => {
    const status = deferred()
    homeStatus = status.promise
    render(App)
    await firstSend('My early draft')
    status.resolve({ configured: true, homePath: '/Saved/Home' })
    await waitFor(() => expect(screen.queryByLabelText('First-run settings')).not.toBeInTheDocument())
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('My early draft')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    await openSettings('Home')
    await fireEvent.click(await screen.findByRole('button', { name: 'Change folder…' }))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Saved/Home')
  })

  it('creates Home on the first Send and preserves the draft through model settings', async () => {
    firstRun()
    render(App)
    await screen.findByText('/Documents/Muniment')
    await firstSend()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    expect(await screen.findByText('No free hosted model exists at the no-account tier.')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(invoke.mock.calls.filter(([command]) => command === 'home_confirm')).toHaveLength(1)
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    expect(await screen.findByPlaceholderText('Ask anything')).toHaveValue('Keep this draft')
    expect(screen.queryByLabelText('First-run settings')).not.toBeInTheDocument()
  })

  it('opens existing model settings without an account and preserves a draft after entry failure', async () => {
    firstRun()
    threadSummaryResult = []
    const original = invoke.getMockImplementation()
    let fail = true
    invoke.mockImplementation((command, payload) => {
      if (command === 'auth_status') return Promise.resolve({ signed_in: false, subject: null })
      if (command === 'local_mode_enter') return fail ? Promise.reject('unavailable') : Promise.resolve()
      if (command === 'local_mode_provider_inventory') return Promise.resolve(emptyInventory)
      return original(command, payload)
    })
    render(App)
    await firstSend()
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('Muniment could not enter local mode. Open model settings again.')
    expect(invoke).toHaveBeenCalledWith('onboarding_model_settings_error', { cause: 'localMode' })
    expect(alert.closest('#onboarding-model-panel')).not.toBeNull()
    expect(alert).toHaveClass('error')
    expect(screen.getByRole('button', { name: 'Open model settings' })).toBeEnabled()
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    fail = false
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    expect(invoke).not.toHaveBeenCalledWith('auth_sign_in')
    expect(invoke.mock.calls.filter(([command]) => command === 'home_confirm')).toHaveLength(1)
  })

  it.each([false, true])('retries startup status after failure and preserves the draft with signed_in=%s', async (signedIn) => {
    firstRun()
    threadSummaryResult = []
    const original = invoke.getMockImplementation()
    let fail = true
    const recovered = deferred()
    invoke.mockImplementation((command, payload) => {
      if (command === 'auth_status') return fail ? Promise.reject('unavailable') : recovered.promise
      if (command === 'local_mode_enter') return Promise.resolve()
      if (command === 'local_mode_provider_inventory') return Promise.resolve(emptyInventory)
      return original(command, payload)
    })
    render(App)
    await firstSend()
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_status')).toHaveLength(2)
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Muniment could not read session status. Open model settings again.')
    expect(invoke).toHaveBeenCalledWith('onboarding_model_settings_error', { cause: 'sessionStatus' })
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_status')).toHaveLength(3)
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    expect(invoke).not.toHaveBeenCalledWith('local_mode_enter')

    fail = false
    const openSettings = screen.getByRole('button', { name: 'Open model settings' })
    await fireEvent.click(openSettings)
    expect(openSettings).toBeDisabled()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).not.toHaveAttribute('aria-disabled', 'true')
    await fireEvent.click(send)
    expect(screen.getByRole('button', { name: 'Open model settings' })).toBe(openSettings)
    await fireEvent.click(openSettings)
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_status')).toHaveLength(4)
    recovered.resolve({ signed_in: signedIn, subject: signedIn ? 'token-subject' : null })
    await waitFor(() => expect(screen.queryByLabelText('First-run settings')).not.toBeInTheDocument())
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    if (signedIn) {
      expect(invoke).not.toHaveBeenCalledWith('local_mode_enter')
      expect(screen.queryByTestId('local-mode')).not.toBeInTheDocument()
    } else {
      expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
      expect(screen.getByTestId('local-mode')).toBeVisible()
      await fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }))
      expect(screen.getByTestId('local-mode')).toBeVisible()
    }
    expect(invoke).not.toHaveBeenCalledWith('auth_sign_in')
    expect(invoke.mock.calls.filter(([command]) => command === 'home_confirm')).toHaveLength(1)
  })

  it('names a failed startup wait and retries startup without losing the draft', async () => {
    firstRun()
    const startup = deferred()
    localModeStatus = startup.promise
    render(App)
    await firstSend()
    let openSettings = screen.getByRole('button', { name: 'Open model settings' })
    await fireEvent.click(openSettings)
    expect(openSettings).toBeDisabled()
    await fireEvent.click(screen.getByTestId('onboarding-home-path'))
    expect(screen.getByTestId('onboarding-picker')).toBeDisabled()
    await fireEvent.click(screen.getByTestId('onboarding-model'))
    openSettings = screen.getByRole('button', { name: 'Open model settings' })
    expect(openSettings).toBeDisabled()
    expect(invoke).not.toHaveBeenCalledWith('auth_status')
    expect(screen.getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled', 'true')

    startup.reject(new Error('marker unavailable'))
    expect(await screen.findByRole('alert')).toHaveTextContent('Muniment could not finish startup. Open model settings again.')
    expect(invoke).toHaveBeenCalledWith('onboarding_model_settings_error', { cause: 'startup' })
    expect(openSettings).toBeEnabled()
    expect(invoke).not.toHaveBeenCalledWith('local_mode_enter')
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')

    localModeStatus = false
    await fireEvent.click(openSettings)
    await waitFor(() => expect(screen.queryByLabelText('First-run settings')).not.toBeInTheDocument())
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    expect(invoke.mock.calls.filter(([command]) => command === 'home_confirm')).toHaveLength(1)
  })

  it('keeps the control when local mode succeeds but the workspace is not ready', async () => {
    firstRun()
    const providers = deferred()
    const original = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => {
      if (command === 'auth_status') return Promise.resolve({ signed_in: false, subject: null })
      if (command === 'local_mode_enter') return Promise.resolve()
      if (command === 'local_mode_provider_inventory') return providers.promise
      return original(command, payload)
    })
    render(App)
    await firstSend()
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('local_mode_provider_inventory'))
    desktopClientListener({ payload: { connected: false, supervisor_running: true } })
    providers.resolve(emptyInventory)
    expect(await screen.findByRole('alert')).toHaveTextContent('The runtime is not connected yet. Open model settings again.')
    expect(screen.getByRole('button', { name: 'Open model settings' })).toBeEnabled()
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    expect(screen.getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled', 'true')
    expect(screen.getByLabelText('First-run settings')).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())

    desktopClientListener({ payload: { connected: true, supervisor_running: true } })
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    expect(screen.queryByLabelText('First-run settings')).not.toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    expect(invoke.mock.calls.filter(([command]) => command === 'local_mode_enter')).toHaveLength(1)
  })

  it('keeps the control after local entry while the first runtime status read stays pending', async () => {
    firstRun()
    const status = deferred()
    const original = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => {
      if (command === 'attach_listener_status') return status.promise
      if (command === 'auth_status') return Promise.resolve({ signed_in: false, subject: null })
      if (command === 'local_mode_enter') return Promise.resolve()
      if (command === 'local_mode_provider_inventory') return Promise.resolve(emptyInventory)
      return original(command, payload)
    })
    render(App)
    await firstSend()
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    expect(invoke).toHaveBeenCalledWith('attach_listener_status')
    expect(invoke).toHaveBeenCalledWith('local_mode_enter')
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('The runtime is not connected yet. Open model settings again.')
    expect(invoke).toHaveBeenCalledWith('onboarding_model_settings_error', { cause: 'runtime' })
    expect(alert.closest('#onboarding-model-panel')).not.toBeNull()
    const openSettings = screen.getByRole('button', { name: 'Open model settings' })
    expect(openSettings).toBeEnabled()
    expect(screen.getByLabelText('First-run settings')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    expect(screen.getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled', 'true')
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())

    await fireEvent.click(openSettings)
    expect(await screen.findByRole('alert')).toHaveTextContent('The runtime is not connected yet. Open model settings again.')
    expect(openSettings).toBeEnabled()
    status.resolve({ connected: true, supervisor_running: true })
    await fireEvent.click(openSettings)
    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    expect(screen.queryByLabelText('First-run settings')).not.toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    expect(invoke.mock.calls.filter(([command]) => command === 'local_mode_enter')).toHaveLength(1)
  })

  it('lists one row per assistant and combines roots without any import mode controls', async () => {
    firstRun()
    scanReport = { findings: [finding('claude', 'Claude Code', 12), finding('pi', 'Pi', 1), finding('pi', 'Pi', 2, { capped: true }), finding('codex', 'Codex CLI', 0)], errors: [] }
    render(App)
    const chip = await screen.findByText('Claude Code: 12 files · Pi: 3 files')
    await fireEvent.click(chip)
    const list = screen.getByRole('list', { name: 'Assistant memory' })
    expect(within(list).getAllByRole('listitem')).toHaveLength(3)
    expect(within(list).getByText('Claude Code: 12 files')).toBeInTheDocument()
    expect(within(list).getByText('Pi: 3 files')).toBeInTheDocument()
    expect(within(list).getByText('Codex CLI: 0 files')).toBeInTheDocument()
    expect(within(list).getByText('Scan cap reached. The count may be incomplete.')).toBeInTheDocument()
    expect(within(list).queryByRole('checkbox')).not.toBeInTheDocument()
    expect(within(list).queryByRole('combobox')).not.toBeInTheDocument()
    expect(invoke.mock.calls.some(([command]) => command.startsWith('onboarding_import'))).toBe(false)
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await firstSend()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
  })

  it('keeps the composer ready while a scan runs and does not reopen a closed panel', async () => {
    firstRun()
    const scan = deferred()
    const original = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => command === 'onboarding_scan' ? scan.promise : original(command, payload))
    render(App)
    await fireEvent.click(screen.getByTestId('onboarding-scan'))
    expect(screen.getByTestId('onboarding-scan')).toHaveTextContent('Scanning assistant memory…')
    await firstSend()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    await fireEvent.click(screen.getByRole('button', { name: 'Open model settings' }))
    scan.resolve()
    await waitFor(() => expect(screen.queryByLabelText('First-run settings')).not.toBeInTheDocument())
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
  })

  it('keeps Home and the draft through scan failure and retry', async () => {
    firstRun()
    const original = invoke.getMockImplementation()
    let fail = true
    invoke.mockImplementation((command, payload) => command === 'onboarding_scan' && fail ? Promise.reject('unavailable') : original(command, payload))
    render(App)
    await fireEvent.click(await screen.findByText('Scan unavailable'))
    expect(screen.getByRole('alert')).toHaveTextContent('Muniment could not scan assistant memory. Retry the scan.')
    await fireEvent.input(screen.getByRole('textbox', { name: 'Message' }), { target: { value: 'My draft' } })
    fail = false
    await fireEvent.click(screen.getByRole('button', { name: 'Retry scan' }))
    expect(await screen.findAllByText('No assistant memory found')).toHaveLength(2)
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('My draft')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
  })

  it('does not label a partial scan as an empty scan', async () => {
    firstRun()
    scanReport = { findings: [finding('pi', 'Pi', 0, { capped: true, warnings: ['timeCap', 'unreadable'] })], errors: [{ assistantId: 'codex', message: 'The root must be absolute.' }] }
    render(App)
    await fireEvent.click(await screen.findByText('Assistant memory scan incomplete'))
    expect(screen.getByText('Pi: 0 files')).toBeInTheDocument()
    expect(screen.getByText('Scan cap reached. The count may be incomplete.')).toBeInTheDocument()
    expect(screen.getByText('Some folders could not be read.')).toBeInTheDocument()
    expect(screen.getByText('codex: The root must be absolute.')).toBeInTheDocument()
    expect(screen.queryByText('No assistant memory found')).not.toBeInTheDocument()
  })

  it('changes Home without writes and leaves the path intact after picker cancellation', async () => {
    firstRun()
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-home-path'))
    const panel = screen.getByRole('region', { name: 'Home' })
    expect(within(panel).getAllByRole('button')).toHaveLength(1)
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    dialogResult = '/Other/Muniment'
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Other/Muniment')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await firstSend()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Other/Muniment' })
  })

  it.each([null, '', [], '/Other/Muniment'])('The Home picker records the resolved value (%s).', async (picked) => {
    firstRun()
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-home-path'))
    const section = screen.getByRole('region', { name: 'First run' })
    expect(JSON.parse(section.getAttribute('data-home-picker'))).toEqual({ status: 'not-started' })
    const dialog = deferred()
    dialogResult = dialog.promise
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    expect(JSON.parse(section.getAttribute('data-home-picker'))).toEqual({ status: 'pending' })
    expect(screen.getByTestId('onboarding-picker')).toBeDisabled()
    dialog.resolve(picked)
    await waitFor(() => expect(JSON.parse(section.getAttribute('data-home-picker'))).toEqual({ status: 'resolved', value: picked }))
    expect(screen.getByTestId('onboarding-picker')).toBeEnabled()
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent(typeof picked === 'string' && picked ? picked : '/Documents/Muniment')
  })

  it.each([new Error('The dialog command failed.'), 'The dialog command failed.'])('The Home picker records a rejection and resets on retry.', async (error) => {
    firstRun()
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-home-path'))
    const section = screen.getByRole('region', { name: 'First run' })
    const dialog = deferred()
    dialogResult = dialog.promise
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    dialog.reject(error)
    await waitFor(() => expect(JSON.parse(section.getAttribute('data-home-picker'))).toEqual({ status: 'rejected', error: 'The dialog command failed.' }))
    expect(screen.getByRole('alert')).toHaveTextContent('Muniment could not open the folder picker.')
    expect(screen.getByTestId('onboarding-picker')).toBeEnabled()
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    const retry = deferred()
    dialogResult = retry.promise
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    expect(JSON.parse(section.getAttribute('data-home-picker'))).toEqual({ status: 'pending' })
    retry.resolve('/Other/Muniment')
    await waitFor(() => expect(JSON.parse(section.getAttribute('data-home-picker'))).toEqual({ status: 'resolved', value: '/Other/Muniment' }))
  })

  it('ignores a stale default after the user chooses Home', async () => {
    const status = deferred()
    homeStatus = status.promise
    dialogResult = '/Other/Muniment'
    render(App)
    await fireEvent.click(screen.getByTestId('onboarding-home-path'))
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    status.resolve({ configured: false, homePath: '/Documents/Muniment' })
    await waitFor(() => expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Other/Muniment'))
    await firstSend()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Other/Muniment' })
  })

  it('recovers from a Home load failure without hiding the composer', async () => {
    homeStatus = Promise.reject('unavailable')
    dialogResult = '/Other/Muniment'
    render(App)
    expect(await screen.findByRole('alert')).toHaveTextContent('Muniment could not read Home.')
    await firstSend()
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    expect(screen.getByRole('button', { name: 'Retry Home' })).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Other/Muniment')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Other/Muniment' })
  })

  it('keeps the draft after a scaffold failure and retries the new folder', async () => {
    firstRun()
    const original = invoke.getMockImplementation()
    let fail = true
    invoke.mockImplementation((command, payload) => command === 'home_confirm' && fail ? Promise.reject('read-only') : original(command, payload))
    render(App)
    await firstSend()
    expect(await screen.findByRole('alert')).toHaveTextContent('Muniment could not create Home.')
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep this draft')
    dialogResult = '/Other/Muniment'
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    fail = false
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(invoke).toHaveBeenLastCalledWith('home_confirm', { homePath: '/Other/Muniment' })
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('ignores empty drafts and duplicate Sends while Home saves', async () => {
    firstRun()
    const saved = deferred()
    const original = invoke.getMockImplementation()
    invoke.mockImplementation((command, payload) => command === 'home_confirm' ? saved.promise : original(command, payload))
    render(App)
    await firstSend('  \n ')
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    const composer = screen.getByRole('textbox', { name: 'Message' })
    await fireEvent.input(composer, { target: { value: 'Hello' } })
    await fireEvent.keyDown(composer, { key: 'Enter', shiftKey: true })
    await fireEvent.keyDown(composer, { key: 'Enter', isComposing: true })
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await fireEvent.keyDown(composer, { key: 'Enter' })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(invoke.mock.calls.filter(([command]) => command === 'home_confirm')).toHaveLength(1)
    saved.resolve({ configured: true, homePath: '/Documents/Muniment' })
    expect(await screen.findByRole('button', { name: 'Open model settings' })).toBeEnabled()
  })

  it('opens each chip panel without changing Home or blocking Send', async () => {
    firstRun()
    render(App)
    for (const id of ['onboarding-model', 'onboarding-home-path', 'onboarding-scan']) {
      const chip = await screen.findByTestId(id)
      await fireEvent.click(chip)
      expect(chip).toHaveAttribute('aria-expanded', 'true')
      expect(screen.getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled', 'true')
      await fireEvent.click(chip)
      expect(chip).toHaveAttribute('aria-expanded', 'false')
    }
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
  })

  it('saves a changed Home from the existing settings control', async () => {
    render(App)
    await openSettings('Home')
    await fireEvent.click(await screen.findByRole('button', { name: 'Change folder…' }))
    dialogResult = '/Other/Home'
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    await fireEvent.click(screen.getByRole('button', { name: 'Save Home' }))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Other/Home' })
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    await openSettings('Home')
    await fireEvent.click(await screen.findByRole('button', { name: 'Change folder…' }))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Other/Home')
  })

  it('keeps a configured Home and cancels settings back to the workspace', async () => {
    homeStatus = { configured: true, homePath: '/Saved/Home' }
    render(App)
    await openSettings('Home')
    await fireEvent.click(await screen.findByRole('button', { name: 'Change folder…' }))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Saved/Home')
    expect(screen.queryByText('Local AI')).not.toBeInTheDocument()
    expect(screen.queryByText('Starting setup')).not.toBeInTheDocument()
    dialogResult = '/Other/Home'
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    await fireEvent.click(screen.getByTestId('onboarding-cancel'))
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await openSettings('Home')
    await fireEvent.click(await screen.findByRole('button', { name: 'Change folder…' }))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Saved/Home')
  })
})


describe('voice dictation', () => {
  it('routes one global press and matching release through the existing dictation lifecycle', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await findWorkspaceComposer()
    await waitFor(() => expect(registerGlobalShortcut).toHaveBeenCalledWith('Control+Shift+Space', expect.any(Function)))

    globalShortcutHandler({ state: 'Released' })
    globalShortcutHandler({ state: 'Pressed' })
    globalShortcutHandler({ state: 'Pressed' })
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1))
    globalShortcutHandler({ state: 'Released' })
    globalShortcutHandler({ state: 'Released' })
    await vi.advanceTimersByTimeAsync(300)

    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
  })

  it('stops a long global hold immediately on release', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await findWorkspaceComposer()
    await waitFor(() => expect(globalShortcutHandler).toBeTypeOf('function'))

    globalShortcutHandler({ state: 'Pressed' })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_start'))
    await vi.advanceTimersByTimeAsync(1_000)
    globalShortcutHandler({ state: 'Released' })

    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
  })

  it('does not treat a tap after a long hold as a rapid double activation', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const voice = await screen.findByRole('button', { name: 'Voice' })

    globalShortcutHandler({ state: 'Pressed' })
    await vi.advanceTimersByTimeAsync(1_000)
    globalShortcutHandler({ state: 'Released' })
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
    await vi.advanceTimersByTimeAsync(100)

    globalShortcutHandler({ state: 'Pressed' })
    globalShortcutHandler({ state: 'Released' })
    await vi.advanceTimersByTimeAsync(300)

    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(2)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(2)
    expect(voice).toHaveAttribute('aria-pressed', 'false')
  })

  it('keeps one global capture running after a rapid double activation and stops on the next activation', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const voice = await screen.findByRole('button', { name: 'Voice' })

    globalShortcutHandler({ state: 'Pressed' })
    globalShortcutHandler({ state: 'Released' })
    await vi.advanceTimersByTimeAsync(150)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(0)
    globalShortcutHandler({ state: 'Pressed' })
    globalShortcutHandler({ state: 'Released' })
    await vi.advanceTimersByTimeAsync(300)

    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(0)
    expect(voice).toHaveAttribute('aria-pressed', 'true')

    globalShortcutHandler({ state: 'Pressed' })
    globalShortcutHandler({ state: 'Released' })
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
    await waitFor(() => expect(voice).toHaveAttribute('aria-pressed', 'false'))
  })

  it('routes rapid click-only activations to hands-free and stops on the next click', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const voice = await screen.findByRole('button', { name: 'Voice' })

    await fireEvent.click(voice)
    await vi.advanceTimersByTimeAsync(150)
    await fireEvent.click(voice)
    await vi.advanceTimersByTimeAsync(300)

    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(0)
    expect(voice).toHaveAttribute('aria-pressed', 'true')

    await fireEvent.click(voice)
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
    await waitFor(() => expect(voice).toHaveAttribute('aria-pressed', 'false'))
  })

  it('retries a quick activation after the first start reaches a terminal status', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    let startCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') {
        startCalls += 1
        return startCalls === 1 ? { state: 'failed', message: 'Could not start' } : { state: 'running' }
      }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const voice = await screen.findByRole('button', { name: 'Voice' })

    await fireEvent.click(voice)
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Could not start'))
    await vi.advanceTimersByTimeAsync(150)
    await fireEvent.click(voice)

    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(2)
    await vi.advanceTimersByTimeAsync(300)
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
    expect(voice).toHaveAttribute('aria-pressed', 'false')
  })

  it('ignores global presses while signed out or a chat is active', async () => {
    let signedIn = false
    let resolveSubmit
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: signedIn, subject: signedIn ? 'token-subject' : undefined }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return new Promise((resolve) => { resolveSubmit = resolve })
      if (command === 'dictation_start') return { state: 'running' }
      throw new Error(`unexpected command: ${command}`)
    })
    const signedOut = render(App)
    await screen.findByRole('button', { name: 'Sign in' })
    globalShortcutHandler({ state: 'Pressed' })
    expect(invoke).not.toHaveBeenCalledWith('dictation_start')
    signedOut.unmount()

    signedIn = true
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    globalShortcutHandler({ state: 'Pressed' })
    expect(invoke).not.toHaveBeenCalledWith('dictation_start')
    resolveSubmit({ runId: 'run-1', attachments: [] })
  })


  it('redacts registration failures and unregisters a successful binding on teardown', async () => {
    registerGlobalShortcut.mockRejectedValueOnce(new Error('Control+Shift+Space owned by SecretApp.exe'))
    const failed = render(App)
    expect(await screen.findByText('The system-wide voice shortcut is unavailable. Voice remains available from the button.')).toBeInTheDocument()
    expect(screen.queryByText(/SecretApp/)).not.toBeInTheDocument()
    failed.unmount()
    expect(unregisterGlobalShortcut).not.toHaveBeenCalled()

    const registered = render(App)
    await waitFor(() => expect(globalShortcutHandler).toEqual(expect.any(Function)))
    registered.unmount()
    await waitFor(() => expect(unregisterGlobalShortcut).toHaveBeenCalledWith('Control+Shift+Space'))
  })

  it('loads a valid saved voice shortcut and ignores malformed saved data', async () => {
    localStorage.setItem('muniment.voice-shortcut', 'Alt+Shift+K')
    const saved = render(App)
    await waitFor(() => expect(registerGlobalShortcut).toHaveBeenCalledWith('Alt+Shift+K', expect.any(Function)))
    expect(await screen.findByRole('button', { name: 'Voice' })).toHaveAttribute('aria-keyshortcuts', 'Alt+Shift+K')
    saved.unmount()
    await waitFor(() => expect(unregisterGlobalShortcut).toHaveBeenCalledWith('Alt+Shift+K'))

    localStorage.setItem('muniment.voice-shortcut', 'no modifiers or secrets')
    registerGlobalShortcut.mockClear()
    render(App)
    await waitFor(() => expect(registerGlobalShortcut).toHaveBeenCalledWith('Control+Shift+Space', expect.any(Function)))
  })

  it('falls back to the working default when a saved shortcut is unavailable', async () => {
    localStorage.setItem('muniment.voice-shortcut', 'Alt+Shift+K')
    registerGlobalShortcut.mockImplementation(async (shortcut, handler) => {
      if (shortcut === 'Alt+Shift+K') throw new Error('private collision details')
      globalShortcutHandler = handler
    })
    render(App)
    await waitFor(() => expect(registerGlobalShortcut).toHaveBeenCalledWith('Control+Shift+Space', expect.any(Function)))
    expect(await screen.findByRole('button', { name: 'Voice' })).toHaveAttribute('aria-keyshortcuts', 'Control+Shift+Space')
    expect(screen.queryByText(/private collision/)).not.toBeInTheDocument()
    expect(screen.queryByText('The system-wide voice shortcut is unavailable. Voice remains available from the button.')).not.toBeInTheDocument()
  })

  it('captures, applies, and restores a modifier-plus-key voice shortcut', async () => {
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.keyDown(composer, { key: 'K', code: 'KeyK', ctrlKey: true, altKey: true })
    expect(registerGlobalShortcut).toHaveBeenCalledTimes(1)
    const dialog = await openSettings('Account')
    const capture = within(dialog).getByRole('button', { name: /Change voice shortcut, current Control\+Shift\+Space/ })

    await fireEvent.click(capture)
    await fireEvent.keyDown(capture, { key: 'k', code: 'KeyK' })
    expect(within(dialog).getByRole('alert')).toHaveTextContent('Include at least one modifier key.')
    await fireEvent.keyDown(capture, { key: 'K', code: 'KeyK', ctrlKey: true, altKey: true })
    expect(capture).toHaveTextContent('Control+Alt+K')
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Apply' }))

    await waitFor(() => expect(localStorage.getItem('muniment.voice-shortcut')).toBe('Control+Alt+K'))
    expect(registerGlobalShortcut).toHaveBeenCalledWith('Control+Alt+K', expect.any(Function))
    expect(unregisterGlobalShortcut).toHaveBeenCalledWith('Control+Shift+Space')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Voice' })).toHaveAttribute('aria-keyshortcuts', 'Control+Alt+K'))

    const restore = within(dialog).getByRole('button', { name: 'Restore default' })
    await waitFor(() => expect(restore).toBeEnabled())
    await fireEvent.click(restore)
    await waitFor(() => expect(localStorage.getItem('muniment.voice-shortcut')).toBe('Control+Shift+Space'))
    await waitFor(() => expect(within(dialog).getByRole('button', { name: /Change voice shortcut, current Control\+Shift\+Space/ })).toBeInTheDocument())
  })

  it('captures the artifact rail chord without toggling the rail', async () => {
    render(App)
    const railToggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    const dialog = await openSettings('Account')
    const capture = within(dialog).getByRole('button', { name: /Change voice shortcut/ })
    const mac = navigator.platform.startsWith('Mac')

    await fireEvent.click(capture)
    await fireEvent.keyDown(capture, { key: 'j', code: 'KeyJ', metaKey: mac, ctrlKey: !mac })

    expect(capture).toHaveTextContent(mac ? 'Meta+J' : 'Control+J')
    expect(railToggle).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByRole('complementary', { name: 'Artifacts' })).not.toBeInTheDocument()
  })

  it('keeps the previous voice binding when replacement registration collides', async () => {
    registerGlobalShortcut.mockImplementation(async (shortcut, handler) => {
      if (shortcut === 'Control+Alt+K') throw new Error('owned by SecretApp.exe')
      globalShortcutHandler = handler
    })
    render(App)
    const dialog = await openSettings('Account')
    const capture = within(dialog).getByRole('button', { name: /Change voice shortcut, current Control\+Shift\+Space/ })
    await fireEvent.click(capture)
    await fireEvent.keyDown(capture, { key: 'K', code: 'KeyK', ctrlKey: true, altKey: true })
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Apply' }))

    expect(await within(dialog).findByRole('alert')).toHaveTextContent('That shortcut is unavailable. Your previous shortcut still works.')
    expect(dialog).not.toHaveTextContent('SecretApp')
    expect(localStorage.getItem('muniment.voice-shortcut')).toBeNull()
    expect(unregisterGlobalShortcut).not.toHaveBeenCalledWith('Control+Shift+Space')
    expect(screen.getByRole('button', { name: 'Voice' })).toHaveAttribute('aria-keyshortcuts', 'Control+Shift+Space')
  })

  it('keeps the registered and displayed shortcut when persistence fails', async () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new DOMException('private storage detail', 'QuotaExceededError') })
    const view = render(App)
    const dialog = await openSettings('Account')
    const capture = within(dialog).getByRole('button', { name: /Change voice shortcut/ })
    await fireEvent.click(capture)
    await fireEvent.keyDown(capture, { key: 'K', code: 'KeyK', ctrlKey: true, altKey: true })
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Apply' }))

    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Your previous shortcut still works.')
    expect(dialog).not.toHaveTextContent('private storage detail')
    expect(registeredShortcuts).toEqual(new Set(['Control+Shift+Space']))
    expect(localStorage.getItem('muniment.voice-shortcut')).toBeNull()
    expect(screen.getByRole('button', { name: 'Voice' })).toHaveAttribute('aria-keyshortcuts', 'Control+Shift+Space')
    setItem.mockRestore()
    view.unmount()
  })

  it('does not claim rollback succeeded when unregistering both bindings fails', async () => {
    unregisterGlobalShortcut.mockImplementation(async (shortcut) => {
      if (['Control+Shift+Space', 'Control+Alt+K'].includes(shortcut)) throw new Error(`private failure for ${shortcut}`)
      registeredShortcuts.delete(shortcut)
    })
    render(App)
    const dialog = await openSettings('Account')
    const capture = within(dialog).getByRole('button', { name: /Change voice shortcut/ })
    await fireEvent.click(capture)
    await fireEvent.keyDown(capture, { key: 'K', code: 'KeyK', ctrlKey: true, altKey: true })
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Apply' }))

    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Voice remains available from the button.')
    expect(dialog).not.toHaveTextContent('private failure')
    expect(registeredShortcuts).toEqual(new Set(['Control+Shift+Space', 'Control+Alt+K']))
    expect(localStorage.getItem('muniment.voice-shortcut')).toBeNull()
    expect(screen.getByRole('button', { name: 'Voice' })).toHaveAttribute('aria-keyshortcuts', 'Control+Shift+Space')
  })

  it('disables rebinding until deferred startup registration settles', async () => {
    const startup = deferred()
    registerGlobalShortcut.mockImplementationOnce(async (shortcut, handler) => {
      await startup.promise
      registeredShortcuts.add(shortcut)
      globalShortcutHandler = handler
    })
    render(App)
    await openSettings('Account')
    const capture = within(screen.getByRole('dialog', { name: 'Settings' })).getByRole('button', { name: /Change voice shortcut/ })
    expect(capture).toBeDisabled()
    await fireEvent.click(capture)
    expect(registerGlobalShortcut).toHaveBeenCalledTimes(1)
    startup.resolve()
    await waitFor(() => expect(capture).toBeEnabled())
    expect(registeredShortcuts).toEqual(new Set(['Control+Shift+Space']))
  })

  it.each(['register', 'unregister'])('cleans every binding when unmounted during replacement %s', async (boundary) => {
    const pending = deferred()
    const view = render(App)
    await waitFor(() => expect(registeredShortcuts).toEqual(new Set(['Control+Shift+Space'])))
    if (boundary === 'register') {
      registerGlobalShortcut.mockImplementationOnce(async (shortcut, handler) => {
        await pending.promise
        registeredShortcuts.add(shortcut)
        globalShortcutHandler = handler
      })
    } else {
      unregisterGlobalShortcut.mockImplementationOnce(async (shortcut) => {
        await pending.promise
        registeredShortcuts.delete(shortcut)
      })
    }
    const dialog = await openSettings('Account')
    const capture = within(dialog).getByRole('button', { name: /Change voice shortcut/ })
    await fireEvent.click(capture)
    await fireEvent.keyDown(capture, { key: 'K', code: 'KeyK', ctrlKey: true, altKey: true })
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Apply' }))
    if (boundary === 'register') await waitFor(() => expect(registerGlobalShortcut).toHaveBeenCalledWith('Control+Alt+K', expect.any(Function)))
    else await waitFor(() => expect(unregisterGlobalShortcut).toHaveBeenCalledWith('Control+Shift+Space'))
    view.unmount()
    pending.resolve()
    await waitFor(() => expect(registeredShortcuts).toEqual(new Set()))
  })






  it('starts on primary pointer down and stops on release while preserving transcript', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    let resolveStop
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return new Promise((resolve) => { resolveStop = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'Before' } })
    const voice = screen.getByRole('button', { name: 'Voice' })

    await fireEvent.pointerDown(voice, { button: 0, pointerId: 1 })
    expect(invoke).toHaveBeenCalledWith('dictation_start')
    dictationListener({ payload: { type: 'transcript', text: 'after' } })
    await fireEvent.pointerUp(voice, { button: 0, pointerId: 1 })
    await vi.advanceTimersByTimeAsync(300)

    resolveStop({ state: 'stopped' })
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(invoke).not.toHaveBeenCalledWith('dictation_polish', expect.anything())
    await vi.advanceTimersByTimeAsync(300)
    expect(voice).toBeEnabled()
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    dictationListener({ payload: { type: 'transcript', text: 'at the boundary' } })

    await waitFor(() => expect(composer).toHaveValue('Before after'))
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1)
    expect(invoke.mock.calls.map(([command]) => command)).not.toContain('dictation_polish')
  })

  it('promotes a rapid button double activation to hands-free and Escape restores the draft during a late start', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    let resolveStart
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return new Promise((resolve) => { resolveStart = resolve })
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'Original draft' } })
    const voice = screen.getByRole('button', { name: 'Voice' })

    await fireEvent.pointerDown(voice, { button: 0, pointerId: 1 })
    await fireEvent.pointerUp(voice, { pointerId: 1 })
    await vi.advanceTimersByTimeAsync(150)
    await fireEvent.pointerDown(voice, { button: 0, pointerId: 2 })
    await fireEvent.pointerUp(voice, { pointerId: 2 })
    dictationListener({ payload: { type: 'transcript', text: 'temporary words' } })
    await vi.advanceTimersByTimeAsync(300)

    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(0)
    expect(voice).toHaveAttribute('aria-pressed', 'true')
    expect(composer).toHaveValue('Original draft temporary words')

    await fireEvent.keyDown(document, { key: 'Escape' })
    expect(composer).toHaveValue('Original draft')
    resolveStart({ state: 'running' })
    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1))
    await waitFor(() => expect(voice).toHaveAttribute('aria-pressed', 'false'))
  })



  it('cancels a primary pointer capture, restores the snapshot, and focuses the composer', async () => {
    let stopping = false
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
    const composer = await findWorkspaceComposer()
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
    const composer = await findWorkspaceComposer()
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
    const composer = await findWorkspaceComposer()
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_status') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'Existing draft' } })
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)

    expect(voice).toHaveAttribute('aria-pressed', 'true')
    expect(screen.getByRole('status')).toHaveTextContent('Starting local dictation…')
    expect(composer).toHaveAccessibleDescription('Starting local dictation…')
    dictationListener({ payload: { type: 'transcript', text: 'spoken words' } })
    await waitFor(() => expect(composer).toHaveValue('Existing draft spoken words'))
    await fireEvent.input(composer, { target: { value: 'Edited transcript' } })
    await stopClickCapture(voice)

    expect(invoke).toHaveBeenCalledWith('dictation_start')
    expect(invoke).toHaveBeenCalledWith('dictation_stop')
    expect(voice).toHaveAttribute('aria-pressed', 'false')
    expect(composer).toHaveValue('Edited transcript')
  })

  it('keeps an active capture stoppable and prevents starting a chat', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
    expect(send).toHaveAttribute('aria-disabled', 'true')
    expect(voice).toBeEnabled()
    await fireEvent.click(send)
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())

    await stopClickCapture(voice)
    expect(invoke).toHaveBeenCalledWith('dictation_stop')
    expect(voice).toHaveAttribute('aria-pressed', 'false')
    await waitFor(() => expect(send).not.toHaveAttribute('aria-disabled'))
  })

  it('renders a terminal failed message and becomes retryable', async () => {
    const message = 'Microphone capture failed.'
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_status') return { state: 'failed', category: 'redacted', message }
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

  it('shows install facts and installs a missing speech model', async () => {
    let installStatusCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'modelNotInstalled', message: 'The speech model is not installed.' }
      if (command === 'parakeet_install_facts') return {
        identity: 'parakeet-tdt-0.6b-v3',
        revision: 'pinned-revision',
        sourceRepository: 'nvidia/parakeet-tdt-0.6b-v3',
        totalDownloadBytes: 672_384_307,
        requiredFreeBytes: 940_819_763,
        speechModelLicense: 'CC BY 4.0',
        voiceActivityModelLicense: 'MIT',
      }
      if (command === 'parakeet_install_start') return { state: 'installing', completedBytes: 0, totalBytes: 672_384_307 }
      if (command === 'parakeet_install_status') {
        installStatusCalls += 1
        if (installStatusCalls === 1) return { state: 'notInstalled' }
        return installStatusCalls === 2
          ? { state: 'installing', completedBytes: 134_476_861, totalBytes: 672_384_307 }
          : { state: 'installed' }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Voice' }))

    const card = await screen.findByRole('region', { name: 'Speech model install' })
    await within(card).findByText('641 MB')
    expect(card).toHaveTextContent('Download641 MB')
    expect(card).toHaveTextContent('Sourcenvidia/parakeet-tdt-0.6b-v3')
    expect(card).toHaveTextContent('Speech model licenseCC BY 4.0')
    expect(card).toHaveTextContent('Voice activity model licenseMIT')
    expect(card).toHaveTextContent('Free disk required897 MB')
    expect(card).not.toHaveTextContent('The speech model is not installed.')

    await fireEvent.click(within(card).getByRole('button', { name: 'Install' }))
    expect(within(card).getByRole('status')).toHaveTextContent('Installing.')
    const progress = within(card).getByRole('progressbar', { name: 'Speech model download progress' })
    expect(progress).toHaveAttribute('value', '134476861')
    expect(progress).toHaveAttribute('max', '672384307')
    expect(card).toHaveTextContent('128 MB / 641 MB')
    // The card leaves once the model is on disk, and the hint carries the next step.
    expect(await screen.findByText('Installed. Press Voice again to dictate.')).toHaveAttribute('role', 'status')
    expect(screen.queryByRole('region', { name: 'Speech model install' })).not.toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('parakeet_install_start')
    expect(invoke.mock.calls.filter(([command]) => command === 'parakeet_install_status')).toHaveLength(3)
    expect(screen.queryByRole('button', { name: 'Install' })).not.toBeInTheDocument()
  })

  it.each([
    ['its close control', async (card) => { await fireEvent.click(within(card).getByRole('button', { name: 'Close speech model install' })) }],
    ['Escape', async (card) => { await fireEvent.keyDown(card, { key: 'Escape' }) }],
  ])('closes the speech model install card with %s and reopens it from Voice', async (_, close) => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'modelNotInstalled', message: 'The speech model is not installed.' }
      if (command === 'parakeet_install_facts') return {
        identity: 'parakeet-tdt-0.6b-v3', revision: 'pinned-revision', sourceRepository: 'nvidia/parakeet-tdt-0.6b-v3',
        totalDownloadBytes: 672_384_307, requiredFreeBytes: 940_819_763, speechModelLicense: 'CC BY 4.0', voiceActivityModelLicense: 'MIT',
      }
      if (command === 'parakeet_install_status') return { state: 'notInstalled' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Voice' }))
    const card = await screen.findByRole('region', { name: 'Speech model install' })
    await within(card).findByText('641 MB')

    await close(card)
    expect(screen.queryByRole('region', { name: 'Speech model install' })).not.toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Voice' }))
    expect(await screen.findByRole('region', { name: 'Speech model install' })).toBeInTheDocument()
  })

  it.each([
    ['cancelled', 'Cancelled.'],
    ['failed', 'Failed.'],
  ])('states a %s install and returns the Install control', async (installState, words) => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'modelNotInstalled', message: 'The speech model is not installed.' }
      if (command === 'parakeet_install_facts') return {
        sourceRepository: 'source', totalDownloadBytes: 1, requiredFreeBytes: 2,
        speechModelLicense: 'CC BY 4.0', voiceActivityModelLicense: 'MIT',
      }
      if (command === 'parakeet_install_status') return { state: installState === 'failed' ? 'notInstalled' : installState }
      if (command === 'parakeet_install_start') return { state: installState }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Voice' }))

    const card = await screen.findByRole('region', { name: 'Speech model install' })
    await within(card).findByText('source')
    if (installState === 'failed') await fireEvent.click(within(card).getByRole('button', { name: 'Install' }))
    expect(within(card).getByRole('status')).toHaveTextContent(words)
    expect(within(card).getByRole('button', { name: 'Install' })).toBeEnabled()
  })

  it('cancels an active speech model install and ignores stale status', async () => {
    let resolveStatus
    const staleStatus = new Promise((resolve) => { resolveStatus = resolve })
    let statusCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'modelNotInstalled', message: 'The speech model is not installed.' }
      if (command === 'parakeet_install_facts') return {
        sourceRepository: 'source', totalDownloadBytes: 1, requiredFreeBytes: 2,
        speechModelLicense: 'CC BY 4.0', voiceActivityModelLicense: 'MIT',
      }
      if (command === 'parakeet_install_status') {
        statusCalls += 1
        return statusCalls === 1
          ? { state: 'installing', completedBytes: 1, totalBytes: 2 }
          : staleStatus
      }
      if (command === 'parakeet_install_cancel') return { state: 'cancelled' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Voice' }))

    const card = await screen.findByRole('region', { name: 'Speech model install' })
    const cancel = await within(card).findByRole('button', { name: 'Cancel install' })
    await waitFor(() => expect(statusCalls).toBe(2))
    await fireEvent.click(cancel)
    await fireEvent.click(cancel)

    expect(invoke.mock.calls.filter(([command]) => command === 'parakeet_install_cancel')).toHaveLength(1)
    expect(await within(card).findByRole('status')).toHaveTextContent('Cancelled.')
    expect(within(card).getByRole('button', { name: 'Install' })).toBeEnabled()

    resolveStatus({ state: 'installed' })
    await Promise.resolve()
    expect(within(card).getByRole('status')).toHaveTextContent('Cancelled.')
  })

  it('polls until an asynchronous speech model cancellation finishes', async () => {
    let resolveStatus
    const cancelledStatus = new Promise((resolve) => { resolveStatus = resolve })
    let statusCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'modelNotInstalled', message: 'The speech model is not installed.' }
      if (command === 'parakeet_install_facts') return {
        sourceRepository: 'source', totalDownloadBytes: 1, requiredFreeBytes: 2,
        speechModelLicense: 'CC BY 4.0', voiceActivityModelLicense: 'MIT',
      }
      if (command === 'parakeet_install_status') {
        statusCalls += 1
        return statusCalls === 1
          ? { state: 'installing', completedBytes: 1, totalBytes: 2 }
          : cancelledStatus
      }
      if (command === 'parakeet_install_cancel') return { state: 'installing', completedBytes: 1, totalBytes: 2 }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Voice' }))

    const card = await screen.findByRole('region', { name: 'Speech model install' })
    const cancel = await within(card).findByRole('button', { name: 'Cancel install' })
    await fireEvent.click(cancel)

    expect(cancel).toBeDisabled()
    resolveStatus({ state: 'cancelled' })
    await waitFor(() => expect(within(card).getByRole('status')).toHaveTextContent('Cancelled.'))
    expect(within(card).getByRole('button', { name: 'Install' })).toBeEnabled()
  })

  it('shows recovery help when speech model cancellation fails', async () => {
    let resolveStatus
    const resumedStatus = new Promise((resolve) => { resolveStatus = resolve })
    let statusCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'modelNotInstalled', message: 'The speech model is not installed.' }
      if (command === 'parakeet_install_facts') return {
        sourceRepository: 'source', totalDownloadBytes: 1, requiredFreeBytes: 2,
        speechModelLicense: 'CC BY 4.0', voiceActivityModelLicense: 'MIT',
      }
      if (command === 'parakeet_install_status') {
        statusCalls += 1
        if (statusCalls === 1) return { state: 'installing', completedBytes: 1, totalBytes: 2 }
        if (statusCalls === 2) return resumedStatus
        return { state: 'cancelled' }
      }
      if (command === 'parakeet_install_cancel') throw new Error('cancel failed')
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Voice' }))

    const card = await screen.findByRole('region', { name: 'Speech model install' })
    await fireEvent.click(await within(card).findByRole('button', { name: 'Cancel install' }))

    expect(await within(card).findByRole('alert')).toHaveTextContent('The speech model install could not be cancelled. Try again.')
    expect(statusCalls).toBe(2)
    resolveStatus({ state: 'installing', completedBytes: 1, totalBytes: 2 })
    await waitFor(() => expect(within(card).getByRole('status')).toHaveTextContent('Cancelled.'))
    expect(statusCalls).toBe(3)
    expect(within(card).getByRole('alert')).toHaveTextContent('The speech model install could not be cancelled. Try again.')
    expect(within(card).getByRole('button', { name: 'Install' })).toBeEnabled()
  })

  it('keeps capture active and stoppable when status IPC rejects', async () => {
    let statusCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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

    await stopClickCapture(voice)
    expect(invoke).toHaveBeenCalledWith('dictation_stop')
    expect(voice).toHaveAttribute('aria-pressed', 'false')
  })

  it('keeps capture active and allows Stop to be retried when stop IPC rejects', async () => {
    let stopCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
    await stopClickCapture(voice)

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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'starting' }
      if (command === 'dictation_status') return { state: 'running' }
      if (command === 'chat_submit') return new Promise((resolve) => { resolveSubmit = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    const view = render(App)
    const composer = await findWorkspaceComposer()
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    view.unmount()
    expect(eventUnlisten).toHaveBeenCalledTimes(6)
    expect(pairingUnlisten).toHaveBeenCalledTimes(1)
    await new Promise((resolve) => setTimeout(resolve, 130))
    expect(invoke).not.toHaveBeenCalledWith('dictation_status')

    render(App)
    const nextComposer = await findWorkspaceComposer()
    await fireEvent.input(nextComposer, { target: { value: 'Question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(screen.getByRole('button', { name: 'Voice' })).toBeDisabled()
    resolveSubmit({ runId: 'run-1' })
  })
})

describe('local file selection', () => {
  it('records a rejected drop listener registration without a stale unlisten handle', async () => {
    dragDropRegistrationError = new Error('sensitive registration detail')
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    const view = render(App)

    await waitFor(() => expect(error).toHaveBeenCalledWith('File drop listener registration failed.'))
    view.unmount()

    expect(dragDropUnlisten).not.toHaveBeenCalled()
    expect(error).not.toHaveBeenCalledWith(expect.stringContaining('sensitive'))
  })

  it('shows and clears the native drop affordance, then de-duplicates dropped files', async () => {
    dialogResult = ['/private/contracts/lease.pdf']
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Add files' }))
    await waitFor(() => expect(dragDropListener).toBeDefined())

    dragDropListener({ payload: { type: 'over', position: { x: 10, y: 10 } } })
    const dropStatus = await screen.findByRole('status')
    expect(dropStatus).toHaveTextContent('Drop files to add themSaved locally · supported images sent with first prompt')
    expect(dropStatus).not.toHaveTextContent(/not sent to (?:the )?model/i)
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

  it('selects an @ file with Enter without sending and closes suggestions with Escape', async () => {
    const original = invoke.getMockImplementation()
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'chat_search_files') return [{ path: '/home/ISSUES.md', displayName: 'ISSUES.md', relativePath: 'ISSUES.md' }]
      return original(command, payload)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /New thread/ }))
    const composer = await findWorkspaceComposer()
    await screen.findByRole('button', { name: 'Add files' })
    await tick()
    await fireEvent.input(composer, { target: { value: 'Read @iss' } })
    composer.setSelectionRange(9, 9)
    await fireEvent.click(composer)
    await screen.findByRole('option', { name: 'ISSUES.md' })
    await fireEvent.keyDown(composer, { key: 'Enter' })
    await waitFor(() => expect(composer).toHaveValue('Read @ISSUES.md '))
    expect(screen.getByRole('button', { name: 'Remove ISSUES.md' })).toBeInTheDocument()
    expect(invoke.mock.calls.some(([command]) => command === 'chat_submit')).toBe(false)
    await fireEvent.input(composer, { target: { value: '@iss', selectionStart: 4 } })
    await screen.findByRole('option', { name: 'ISSUES.md' })
    await fireEvent.keyDown(composer, { key: 'Escape' })
    expect(screen.queryByRole('listbox', { name: 'File suggestions' })).not.toBeInTheDocument()
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

  it.each([
    ['a pinned transcript', 200, 300, 200],
    ['a transcript scrolled up', 75, 75, 75],
  ])('restores %s after files are added and removed', async (_case, initial, withFiles, withoutFiles) => {
    dialogResult = ['/private/contracts/lease.pdf', '/private/notes.txt']
    render(App)
    const thread = await screen.findByRole('region', { name: /Transcript:/ })
    Object.defineProperties(thread, {
      scrollHeight: { configurable: true, value: 600 },
      clientHeight: {
        configurable: true,
        get: () => screen.queryByRole('list', { name: 'Selected files' }) ? 300 : 400,
      },
    })
    thread.scrollTop = 200
    await fireEvent.scroll(thread)
    if (initial !== 200) {
      thread.scrollTop = initial
      await fireEvent.scroll(thread)
    }

    await fireEvent.click(screen.getByRole('button', { name: 'Add files' }))
    await screen.findByRole('list', { name: 'Selected files' })
    await waitFor(() => expect(thread.scrollTop).toBe(withFiles))

    await fireEvent.click(screen.getByRole('button', { name: 'Remove lease.pdf' }))
    await waitFor(() => expect(thread.scrollTop).toBe(withFiles))

    await fireEvent.click(screen.getByRole('button', { name: 'Remove notes.txt' }))
    await waitFor(() => expect(thread.scrollTop).toBe(withoutFiles))
  })

  it('retains draft and selection when local ingestion fails', async () => {
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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

    const error = await within(document.querySelector('.thread')).findByText('One or more selected files could not be added.')
    expect(error).toHaveClass('run-error')
    expect(screen.getByTestId('run-announcement')).toHaveTextContent('One or more selected files could not be added.')
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'chat_file_metadata') {
        return { displayName: payload.path.split('/').pop(), byteLength: 1024 }
      }
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return new Promise((resolve) => { resolveSubmit = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    dialogResult = ['/private/contracts/lease.png', '/private/notes.txt']
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Add files' }))
    const composer = screen.getByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Review these' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    expect(invoke).toHaveBeenCalledWith('chat_submit', {
      prompt: 'Review these',
      files: [
        { path: '/private/contracts/lease.png' },
        { path: '/private/notes.txt' },
      ],
    })
    expect(composer).toHaveValue('Review these')
    expect(screen.getByText('lease.png')).toBeInTheDocument()
    expect(screen.getByText('notes.txt')).toBeInTheDocument()

    resolveSubmit({ runId: 'run-with-files', attachments: [
      { displayName: 'lease.png', byteLength: 1024, mediaType: 'image/png' },
      { displayName: 'notes.txt', byteLength: 1024 },
    ] })
    await waitFor(() => expect(composer).toHaveValue(''))
    expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument()
    const saved = screen.getByRole('list', { name: 'Saved attachments' })
    const chips = within(saved).getAllByRole('listitem')
    expect(chips[0]).toHaveTextContent('lease.png1.0 KBimage/png')
    expect(chips[1]).toHaveTextContent('notes.txt1.0 KB')
    expect(chips[1]).not.toHaveTextContent('image/')
    expect(screen.getAllByText('Supported images are sent with the first prompt.')).toHaveLength(1)
    expect(saved).not.toHaveTextContent(/not sent to (?:the )?model/i)
    expect(document.body).not.toHaveTextContent('/private/contracts')
  })
})

it('hydrates safe durable attachment chips without paths or hashes', async () => {
  invoke.mockImplementation(async (command) => {
    if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_thread_open') return [{
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
  expect(saved).toHaveTextContent('contract.pdf214 KB')
  expect(within(saved).queryByText(/image\//)).not.toBeInTheDocument()
  expect(screen.getAllByText('Supported images are sent with the first prompt.')).toHaveLength(1)
  expect(saved).not.toHaveTextContent(/not sent to (?:the )?model/i)
  expect(document.body).not.toHaveTextContent('/private/contract.pdf')
  expect(document.body).not.toHaveTextContent('sha256')
})

it('hydrates durable attachment chips when the prompt is unavailable', async () => {
  invoke.mockImplementation(async (command) => {
    if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_thread_open') return [{
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
  expect(saved).toHaveTextContent('evidence.txt1.5 KB')
  expect(within(saved).queryByText(/image\//)).not.toBeInTheDocument()
  expect(saved).not.toHaveTextContent(/not sent to (?:the )?model/i)
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

  it('renders each applied diff and the unavailable sentence', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-1', phase: 'complete', text: 'Done', receipt: null, toolActivity: [],
        appliedDiffs: [
          { effectId: 'effect-1', codeDiffId: 'fixture-modified', diff: modifiedCodeDiff },
          { effectId: 'effect-2', codeDiffId: 'missing', diff: null },
        ],
      }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    const headings = await screen.findAllByText('Applied file changes')
    expect(headings).toHaveLength(2)
    expect(screen.getByLabelText('Code changes')).toHaveAttribute('data-diff-id', 'fixture-modified')
    expect(screen.getByText('The changes were applied, but their record is no longer stored.')).toBeInTheDocument()
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return interrupted()
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
    expect(screen.queryByRole('button', { name: 'Send' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Stop' })).toHaveAttribute('aria-disabled', 'true')
    await fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(invoke).not.toHaveBeenCalledWith('chat_cancel', expect.anything())
    expect(invoke.mock.calls.filter(([command]) => command === 'chat_resume')).toEqual([
      ['chat_resume', { runId: 'run-interrupted' }],
    ])
    resolveResume({ runId: 'run-interrupted' })
  })

  it('does not regress live completion when resume invocation resolves later', async () => {
    let resolveResume
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return interrupted()
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return interrupted(false)
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
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return interrupted()
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

describe('chat submission settlement', () => {
  const existingRun = {
    runId: 'existing-run', phase: 'complete', text: 'Existing answer',
    prompt: 'Existing question', receipt: {}, toolActivity: [],
  }

  function expectNoProxyEqualityWarning(warn) {
    expect(warn.mock.calls.flat().join(' ')).not.toContain('state_proxy_equality_mismatch')
  }

  it('replaces only the matching pending run after a successful submission', async () => {
    let resolveSubmit
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [existingRun]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return new Promise((resolve) => { resolveSubmit = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'New question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    resolveSubmit({ runId: 'new-run', attachments: [] })
    await waitFor(() => expect(composer).toHaveValue(''))
    chatListener({ payload: {
      runId: 'new-run', phase: 'complete', text: 'New answer',
      receipt: {}, toolActivity: [], pendingPermission: null,
    } })

    expect(await screen.findByText('New answer')).toBeInTheDocument()
    expect(screen.getByText('Existing answer')).toBeInTheDocument()
    expectNoProxyEqualityWarning(warn)
  })

  it.each([
    'The submission was rejected.',
    'chat_not_entitled: No chat model is currently available for this account.',
  ])('replaces only the matching pending run after a failed submission: %s', async (reason) => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [existingRun]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') throw reason
      throw new Error(`unexpected command: ${command}`)
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: 'New question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    const thread = within(document.querySelector('.thread'))
    const error = await thread.findByText(reason)
    expect(error).toHaveClass('run-error')
    expect(within(error).getByRole('button', { name: 'Try again' })).toBeEnabled()
    expect(screen.getByTestId('run-announcement')).toHaveTextContent(reason)
    expect(screen.queryByText('Muniment cannot reach its background service.')).not.toBeInTheDocument()
    expect(document.querySelector('.cancel-error')).not.toBeInTheDocument()
    expect(composer).toHaveAccessibleDescription('Routing is automatic. Every reply carries its receipt.')
    expect(thread.getByText('Existing answer')).toBeInTheDocument()
    expectNoProxyEqualityWarning(warn)
  })
})

describe('permission gates', () => {
  function restoreGate(gate, phase = 'pending-permission', answer = vi.fn().mockResolvedValue(undefined)) {
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') {
        return [{
          runId: 'run-gated',
          phase,
          text: 'I need access.',
          prompt: 'Help with this file.',
          receipt: null,
          toolActivity: [],
          pendingPermission: gate,
        }]
      }
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_answer_permission') return answer(payload)
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    return answer
  }

  it('renders a confirm request with its title, message, and two choices', async () => {
    restoreGate({
      gateId: 'gate-confirm',
      kind: 'confirm',
      title: 'Run a command',
      message: 'rm /tmp/draft',
    })

    const card = await screen.findByText('Run a command')
    expect(card.closest('.permission-card')).toHaveClass('tool-card')
    expect(screen.getByText('rm /tmp/draft')).toBeInTheDocument()
    const allow = screen.getByRole('button', { name: 'Allow' })
    const deny = screen.getByRole('button', { name: 'Deny' })
    expect(allow.closest('.permission-approve-actions')).not.toContainElement(deny)
  })

  it('renders no request outside the pending permission phase', async () => {
    restoreGate({ gateId: 'gate-hidden', kind: 'confirm', title: 'Hidden request', message: 'A path' }, 'streaming')

    expect(await screen.findByText('I need access.')).toBeInTheDocument()
    expect(screen.queryByText('Hidden request')).not.toBeInTheDocument()
  })

  it('sends a typed confirm answer and disables every choice until it settles', async () => {
    const pending = deferred()
    const answer = restoreGate(
      { gateId: 'gate-confirm', kind: 'confirm', title: 'Run a command', message: 'rm /tmp/draft' },
      'pending-permission',
      vi.fn(() => pending.promise),
    )

    const allow = await screen.findByRole('button', { name: 'Allow' })
    const deny = screen.getByRole('button', { name: 'Deny' })
    await fireEvent.click(allow)

    expect(answer).toHaveBeenCalledWith({
      runId: 'run-gated',
      gateId: 'gate-confirm',
      answer: { type: 'confirm', value: true },
    })
    expect(allow).toBeDisabled()
    expect(deny).toBeDisabled()

    pending.resolve()
    await waitFor(() => expect(allow).toBeEnabled())
    expect(deny).toBeEnabled()
  })

  it('sends each select option and Deny with their typed answers', async () => {
    const answer = restoreGate({
      gateId: 'gate-select',
      kind: 'select',
      title: 'Choose access',
      options: ['Allow once', 'Allow for this thread'],
    })

    const options = await screen.findByRole('group', { name: 'Choose access' })
    const selected = within(options).getByRole('button', { name: 'Allow for this thread' })
    const deny = screen.getByRole('button', { name: 'Deny' })
    expect(options).not.toContainElement(deny)

    await fireEvent.click(selected)
    expect(answer).toHaveBeenLastCalledWith({
      runId: 'run-gated',
      gateId: 'gate-select',
      answer: { type: 'select', value: 'Allow for this thread' },
    })

    await fireEvent.click(deny)
    expect(answer).toHaveBeenLastCalledWith({
      runId: 'run-gated',
      gateId: 'gate-select',
      answer: { type: 'cancelled' },
    })
  })

  it('renders Deny alone for an unknown request', async () => {
    const kind = 'unknown'
    restoreGate({ gateId: `gate-${kind}`, kind, title: 'Unsupported request' })

    expect(await screen.findByRole('button', { name: 'Deny' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Allow' })).not.toBeInTheDocument()
  })

  it('renders and applies a stored code diff with its bound identifiers', async () => {
    const answer = restoreGate({
      gateId: 'gate-code-diff',
      kind: 'code_diff',
      effect_id: 'effect-1',
      code_diff_id: 'fixture-modified',
      diff_sha256: 'diff-hash',
      write_plan_sha256: 'plan-hash',
      diff: modifiedCodeDiff,
    })

    const title = await screen.findByText('Proposed file changes')
    const card = title.closest('.permission-card')
    expect(within(card).getByLabelText('Code changes')).toHaveAttribute('data-diff-id', 'fixture-modified')
    expect(within(card).getByText((_, element) => element.classList.contains('d2h-code-line-ctn')
      && element.textContent === 'Hello Muniment')).toBeInTheDocument()
    const approve = within(card).getByRole('button', { name: 'Apply' })
    const deny = within(card).getByRole('button', { name: 'Deny' })
    expect(approve.closest('.permission-approve-actions')).not.toContainElement(deny)

    await fireEvent.click(approve)
    expect(answer).toHaveBeenCalledWith({
      runId: 'run-gated',
      gateId: 'gate-code-diff',
      answer: {
        type: 'codeDiff',
        value: {
          gate_id: 'gate-code-diff',
          effect_id: 'effect-1',
          code_diff_id: 'fixture-modified',
          diff_sha256: 'diff-hash',
          write_plan_sha256: 'plan-hash',
        },
      },
    })
  })

  it('offers only Deny when a code diff gate has no stored diff', async () => {
    restoreGate({
      gateId: 'gate-code-diff',
      kind: 'code_diff',
      effect_id: 'effect-1',
      code_diff_id: 'diff-1',
      diff_sha256: 'diff-hash',
      write_plan_sha256: 'plan-hash',
    })

    const title = await screen.findByText('Proposed file changes')
    const card = title.closest('.permission-card')
    expect(within(card).getByText('Muniment will not apply a change it cannot show. Deny is the only choice.')).toBeInTheDocument()
    expect(within(card).getByRole('button', { name: 'Deny' })).toBeInTheDocument()
    expect(within(card).queryByRole('button', { name: 'Apply' })).not.toBeInTheDocument()
    expect(within(card).queryByLabelText('Code changes')).not.toBeInTheDocument()
  })

  it.each([
    ['input', { placeholder: 'Type a folder name' }, '', 'Quarterly records'],
    ['editor', { prefill: 'rm old.csv\n' }, 'rm old.csv\n', 'archive old.csv\nremove temp.csv'],
  ])('renders and submits a %s request', async (kind, fields, initialValue, submittedValue) => {
    const answer = restoreGate({
      gateId: `gate-${kind}`,
      kind,
      title: 'Change the request',
      ...fields,
    })

    const field = await screen.findByRole('textbox', { name: 'Change the request' })
    expect(field).toHaveValue(initialValue)
    if (kind === 'input') expect(field).toHaveAttribute('placeholder', fields.placeholder)

    await fireEvent.input(field, { target: { value: submittedValue } })
    await fireEvent.click(screen.getByRole('button', { name: 'Submit' }))

    expect(answer).toHaveBeenCalledWith({
      runId: 'run-gated',
      gateId: `gate-${kind}`,
      answer: { type: kind, value: submittedValue },
    })
  })

  it('submits an input request from the keyboard with its current value', async () => {
    const answer = restoreGate({
      gateId: 'gate-input',
      kind: 'input',
      title: 'Change the request',
      placeholder: 'Type a folder name',
    })
    const field = await screen.findByRole('textbox', { name: 'Change the request' })
    await fireEvent.input(field, { target: { value: 'Quarterly records' } })
    const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })

    field.dispatchEvent(enter)

    expect(enter.defaultPrevented).toBe(true)
    expect(answer).toHaveBeenCalledTimes(1)
    expect(answer).toHaveBeenCalledWith({
      runId: 'run-gated',
      gateId: 'gate-input',
      answer: { type: 'input', value: 'Quarterly records' },
    })
  })

  it('keeps editor Enter native and submits its current value with the platform chord', async () => {
    const answer = restoreGate({
      gateId: 'gate-editor',
      kind: 'editor',
      title: 'Change the request',
      prefill: 'rm old.csv',
    })
    const field = await screen.findByRole('textbox', { name: 'Change the request' })
    const hint = screen.getByText(navigator.platform.startsWith('Mac') ? '⌘ ⏎ submits' : 'Ctrl ⏎ submits')
    expect(hint).toHaveClass('permission-editor-hint')
    expect(field).toHaveAttribute('aria-describedby', hint.id)
    await fireEvent.input(field, { target: { value: 'archive old.csv' } })
    const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    field.dispatchEvent(enter)

    expect(enter.defaultPrevented).toBe(false)
    expect(answer).not.toHaveBeenCalled()

    const mac = navigator.platform.startsWith('Mac')
    const chord = new KeyboardEvent('keydown', {
      key: 'Enter',
      metaKey: mac,
      ctrlKey: !mac,
      bubbles: true,
      cancelable: true,
    })
    field.dispatchEvent(chord)

    expect(chord.defaultPrevented).toBe(true)
    expect(answer).toHaveBeenCalledTimes(1)
    expect(answer).toHaveBeenCalledWith({
      runId: 'run-gated',
      gateId: 'gate-editor',
      answer: { type: 'editor', value: 'archive old.csv' },
    })
  })

  it.each([
    ['input', { placeholder: 'Type a folder name' }, 'Quarterly records'],
    ['editor', { prefill: 'rm old.csv' }, 'archive old.csv'],
  ])('disables a %s request in flight and keeps its value after failure', async (kind, fields, typedValue) => {
    const pending = deferred()
    restoreGate(
      { gateId: `gate-${kind}`, kind, title: 'Change the request', ...fields },
      'pending-permission',
      vi.fn(() => pending.promise),
    )

    const field = await screen.findByRole('textbox', { name: 'Change the request' })
    await fireEvent.input(field, { target: { value: typedValue } })
    await fireEvent.click(screen.getByRole('button', { name: 'Submit' }))

    expect(field).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Submit' })).toBeDisabled()
    pending.reject(new Error('offline'))

    expect(await screen.findByRole('alert')).toHaveTextContent('Could not answer this request. Try again.')
    expect(field).toBeEnabled()
    expect(field).toHaveValue(typedValue)
  })

  it('keeps a rejected request, states the failure, and accepts a retry', async () => {
    const answer = restoreGate(
      { gateId: 'gate-retry', kind: 'confirm', title: 'Run a command', message: 'rm /tmp/draft' },
      'pending-permission',
      vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue(undefined),
    )

    await fireEvent.click(await screen.findByRole('button', { name: 'Allow' }))

    const failure = await screen.findByRole('alert')
    expect(failure).toHaveClass('run-error')
    expect(failure).toHaveTextContent('Could not answer this request. Try again.')
    expect(screen.getByText('Run a command')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Allow' })).toBeEnabled()

    await fireEvent.click(screen.getByRole('button', { name: 'Allow' }))
    expect(answer).toHaveBeenCalledTimes(2)
    await waitFor(() => expect(screen.queryByText('Could not answer this request. Try again.')).not.toBeInTheDocument())
  })
})

describe('thread announcements', () => {
  const restored = [{
    runId: 'run-old', phase: 'complete', text: 'Restored answer',
    prompt: 'Old question', receipt: {}, toolActivity: [],
  }]


it('prepares another-model retry without sending until the model choice is saved', async () => {
  localModeStatus = true
  signedIn([{ runId: 'old', phase: 'complete', text: 'Earlier answer', prompt: 'Check the file', receipt: { model: 'ollama/first', tools: [{ name: 'write', calls: 1 }] } }], { runId: 'new', attachments: [] })
  const base = invoke.getMockImplementation()
  const saved = deferred()
  invoke.mockImplementation((command, payload) => {
    if (command === 'local_mode_provider_inventory') return Promise.resolve({ default_provider: 'ollama', default_model: 'first', hidden: [], providers: [{ id: 'ollama', source: 'local', models: [{ id: 'first' }, { id: 'second' }] }] })
    if (command === 'local_mode_set_default_model') return saved.promise
    return base(command, payload)
  })
  await screen.findByText('Earlier answer')
  await fireEvent.click(screen.getByRole('button', { name: 'Retry with another model' }))
  expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
  expect(screen.getByRole('region', { name: 'Review retry' })).toHaveTextContent('including earlier replies and tool results')
  await fireEvent.click(within(screen.getByRole('dialog', { name: 'Model', exact: true })).getByRole('button', { name: 'second' }))
  await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
  expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
  saved.resolve()
  await waitFor(() => expect(screen.getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled', 'true'))
  await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
  expect(invoke).toHaveBeenCalledWith('chat_submit', { prompt: 'Check the file', files: [] })
})

  function signedIn(history, submit) {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return history
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit' && submit) return submit
      throw new Error(`unexpected command: ${command}`)
    })
    return render(App)
  }

  // Screen readers speak on content change, so the honest assertion is that the
  // region's DOM does not change at all while chunks land.
  function watch(region) {
    const seen = []
    const observer = new MutationObserver((records) => seen.push(...records))
    observer.observe(region, { childList: true, characterData: true, subtree: true })
    return () => {
      seen.push(...observer.takeRecords())
      return seen.splice(0)
    }
  }

  it.each([
    ['streaming', 1],
    ['thinking', 0],
    ['complete', 0],
    ['failed', 0],
    ['interrupted', 0],
  ])('renders the underscore caret only in the %s phase', async (phase, ruleCount) => {
    const { container } = signedIn([{
      runId: `run-${phase}`,
      phase,
      text: 'A response long enough to represent prose.',
      prompt: 'A question',
      receipt: phase === 'complete' ? {} : null,
      toolActivity: [],
      resumable: false,
    }])

    // A restored run in thought with text already written reads Thinking.
    if (phase === 'thinking') await screen.findByLabelText('Thinking')
    else await screen.findByText('A response long enough to represent prose.')
    expect(container.querySelectorAll('.caret')).toHaveLength(ruleCount)
  })

  it('renders Markdown from the first streamed token with the caret in its last block, then settles it', async () => {
    const { container } = signedIn([{
      runId: 'run-markdown', phase: 'streaming', text: '## Draft\n\nA first line',
      prompt: 'A question', receipt: null, toolActivity: [], resumable: false,
    }])

    const heading = await screen.findByRole('heading', { name: 'Draft' })
    expect(heading).toHaveProperty('tagName', 'H3')
    const streaming = container.querySelector('.streaming')
    expect(streaming).toContainElement(heading)
    expect(streaming.querySelector('.assistant-markdown')).toBeInTheDocument()
    const caret = container.querySelector('.caret')
    expect(caret.parentElement).toHaveProperty('tagName', 'P')
    expect(caret.parentElement).toHaveTextContent('A first line')
    expect(caret).toHaveTextContent('_')
    expect(streaming.querySelector('.streaming-rule')).not.toBeInTheDocument()
    expect(screen.queryByText('## Draft')).not.toBeInTheDocument()

    chatListener({ payload: {
      runId: 'run-markdown', phase: 'complete', text: '## Final',
      receipt: {}, toolActivity: [],
    } })

    expect(await screen.findByRole('heading', { name: 'Final' })).toHaveProperty('tagName', 'H3')
    expect(container.querySelector('.assistant-markdown')).toBeInTheDocument()
    expect(container.querySelector('.streaming')).not.toBeInTheDocument()
    expect(container.querySelector('.caret')).not.toBeInTheDocument()
    expect(container.querySelector('.streaming-rule')).not.toBeInTheDocument()
  })

  it('renders a paused reply as Markdown too', async () => {
    const { container } = signedIn([{
      runId: 'run-paused', phase: 'pending-permission', text: '## Not terminal',
      prompt: 'A question', receipt: null, toolActivity: [], resumable: false,
      pendingPermission: null,
    }])

    expect(await screen.findByRole('heading', { name: 'Not terminal' })).toHaveProperty('tagName', 'H3')
    expect(container.querySelector('.assistant-markdown')).toBeInTheDocument()
    expect(container.querySelector('.caret')).not.toBeInTheDocument()
  })

  it('keeps the mark through streaming, reads Writing after the hold, and settles it with the reply', async () => {
    signedIn([{
      runId: 'run-thinking', phase: 'thinking', text: '', prompt: 'A question',
      receipt: null, toolActivity: [], resumable: false,
    }])
    const chip = await screen.findByText('Routing')
    const mark = screen.getByLabelText('Routing').querySelector('path')
    expect(mark).toHaveAttribute('fill-rule', 'evenodd')
    expect(mark).not.toHaveAttribute('stroke')

    chatListener({ payload: {
      runId: 'run-thinking', phase: 'streaming', text: 'First token',
      receipt: null, toolActivity: [], pendingPermission: null,
    } })

    expect(chip).toBeInTheDocument()
    expect(await screen.findByText('First token')).toBeInTheDocument()
    await waitFor(() => expect(chip).toHaveTextContent('Writing'), { timeout: 1500 })
    expect(screen.getByLabelText('Writing')).toBeInTheDocument()

    chatListener({ payload: {
      runId: 'run-thinking', phase: 'complete', text: 'First token',
      receipt: {}, toolActivity: [], pendingPermission: null,
    } })
    await waitFor(() => expect(chip).not.toBeInTheDocument(), { timeout: 500 })
  })

  it('names the running tool by its verb and returns to Thinking when it ends', async () => {
    signedIn([{
      runId: 'run-tool', phase: 'thinking', text: '', prompt: 'A question',
      receipt: null, toolActivity: [], resumable: false,
    }])
    const chip = await screen.findByText('Routing')
    chatListener({ payload: {
      runId: 'run-tool', phase: 'thinking', text: '', turnStarted: true,
      receipt: null, toolActivity: [{ effectId: 'e1', displayName: 'grep', status: 'running' }], pendingPermission: null,
    } })
    await waitFor(() => expect(chip).toHaveTextContent('Reading'), { timeout: 1500 })
    chatListener({ payload: {
      runId: 'run-tool', phase: 'thinking', text: '', turnStarted: true,
      receipt: null, toolActivity: [{ effectId: 'e1', displayName: 'grep', status: 'completed' }], pendingPermission: null,
    } })
    await waitFor(() => expect(chip).toHaveTextContent('Thinking'), { timeout: 1500 })
  })

  it('removes the mark without motion when reduced motion is preferred', async () => {
    const originalMatchMedia = window.matchMedia
    window.matchMedia = vi.fn(() => ({ matches: true }))
    try {
      signedIn([{
        runId: 'run-reduced', phase: 'thinking', text: '', prompt: 'A question',
        receipt: null, toolActivity: [], resumable: false,
      }])
      const chip = await screen.findByText('Routing')

      chatListener({ payload: {
        runId: 'run-reduced', phase: 'complete', text: 'First token',
        receipt: {}, toolActivity: [], pendingPermission: null,
      } })

      await waitFor(() => expect(chip).not.toBeInTheDocument())
      expect(screen.getByText('First token')).toBeInTheDocument()
    } finally {
      window.matchMedia = originalMatchMedia
    }
  })

  it('keeps the transcript out of the live region and stays silent when history is restored', async () => {
    signedIn(restored)

    expect(await screen.findByText('Restored answer')).toBeInTheDocument()
    const thread = document.querySelector('.thread')
    expect(thread).not.toHaveAttribute('aria-live')
    expect(thread).toHaveAttribute('role', 'region')
    expect(thread).toHaveAccessibleName('Transcript: Old question')
    const region = screen.getByTestId('run-announcement')
    expect(thread).not.toContainElement(region)
    expect(document.querySelectorAll('.thread-shell [aria-live]')).toHaveLength(1)
    expect(region).toHaveAttribute('aria-live', 'polite')
    expect(region).toHaveAttribute('aria-atomic', 'true')
    expect(region).toHaveClass('visually-hidden')
    expect(region.textContent).toBe('')
  })

  it.each([
    ['', 'No reply arrived within 30 seconds.', 'Try again.'],
    ['Partial answer', 'No reply arrived within 30 seconds.', 'Try again.'],
    ['', 'Acme Inc. logo.png exceeds the 10 MB image limit.', ''],
    ['', 'Acme Inc. logo.png exceeds the 10 MB image limit.', 'Choose a smaller image before sending again.'],
  ])('shows a restored failure cause once beside retry with reply text %j and cause %j', async (text, cause, guidance) => {
    signedIn([{
      runId: 'run-failed', phase: 'failed', text, prompt: 'A question',
      failureReason: `${cause} ${guidance}`.trim(),
    }], { runId: 'retry-run', attachments: [] })
    const error = await screen.findByText(cause)
    expect(error).toHaveClass('run-error')
    expect(screen.getAllByText(cause)).toHaveLength(1)
    expect(screen.getByTestId('run-announcement').textContent).toBe('')
    if (text) expect(screen.getByText(text)).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Ask anything')).toHaveAccessibleDescription('Routing is automatic. Every reply carries its receipt.')
    const retry = within(error).getByRole('button', { name: 'Try again' })
    await fireEvent.click(retry)
    expect(invoke).not.toHaveBeenCalledWith('chat_submit', expect.anything())
    expect(screen.getByRole('region', { name: 'Review retry' })).toHaveTextContent('Sending may run tools again.')
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(invoke).toHaveBeenCalledWith('chat_submit', { prompt: 'A question', files: [] })
    expect(retry).toBeDisabled()
  })

  it.each(['live', 'restored'])('shows the %s prompt storage notice in one mono line without blocking the reply', async (source) => {
    localModeStatus = true
    const promptStorageNotice = 'Prompt text stays in runtime memory for this run. Keyring error -25307: A default keychain could not be found.'
    const entry = { runId: 'memory-run', phase: 'streaming', text: 'First reply', promptStorageNotice }
    if (source === 'restored') {
      signedIn([{ ...entry, prompt: null }])
    } else {
      signedIn([], { runId: entry.runId, attachments: [] })
      const composer = await screen.findByPlaceholderText('Ask anything')
      await fireEvent.input(composer, { target: { value: 'A question' } })
      await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
      chatListener({ payload: entry })
    }
    const summary = await screen.findByText('Prompt text stays in runtime memory for this run.')
    expect(summary.tagName).toBe('SUMMARY')
    expect(summary).not.toHaveAttribute('title')
    expect(summary.closest('details')).toHaveClass('prompt-storage-notice')
    expect(appRules.get('.prompt-storage-notice')).toContain('var(--font-mono)')
    expect(appRules.get('.prompt-storage-notice summary')).toContain('white-space: nowrap')
    await fireEvent.click(summary)
    expect(screen.getByText(promptStorageNotice)).toBeVisible()
    expect(await screen.findByText('First reply')).toBeInTheDocument()
    expect(screen.queryByText('Conversation history is unavailable.')).not.toBeInTheDocument()
    chatListener({ payload: { ...entry, phase: 'complete', text: 'Complete reply' } })
    expect(await screen.findByText('Complete reply')).toBeInTheDocument()
    expect(screen.getAllByText(promptStorageNotice)).toHaveLength(1)
    expect(screen.getByPlaceholderText('Ask anything')).toBeEnabled()
  })

  it('keeps retry disabled when the failed run has no saved prompt', async () => {
    signedIn([{ runId: 'missing-prompt', phase: 'failed', text: '', failureReason: 'The provider refused access.' }])
    const error = await screen.findByText('The provider refused access.')
    expect(within(error).getByRole('button', { name: 'Try again' })).toBeDisabled()
  })

  it.each([
    [false, 'No reply arrived within 30 seconds.', 'Try again.'],
    [true, 'No reply arrived within 30 seconds.', 'Try again.'],
    [false, 'Acme Inc. logo.png exceeds the 10 MB image limit.', ''],
    [true, 'Acme Inc. logo.png exceeds the 10 MB image limit.', 'Choose a smaller image before sending again.'],
  ])('shows a live failure cause only in the run when local mode is %s and cause is %j', async (local, cause, guidance) => {
    localModeStatus = local
    signedIn([], { runId: 'run-failed', attachments: [] })
    const composer = await screen.findByPlaceholderText('Ask anything')
    const hint = local ? '' : 'Routing is automatic. Every reply carries its receipt.'
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    chatListener({ payload: {
      runId: 'run-failed', phase: 'failed', text: '',
      failureReason: `${cause} ${guidance}`.trim(),
    } })
    const thread = within(document.querySelector('.thread'))
    const error = await thread.findByText(cause)
    expect(thread.getAllByText(cause)).toHaveLength(1)
    expect(within(error).getByRole('button', { name: 'Try again' })).toBeEnabled()
    expect(screen.getByTestId('run-announcement').textContent).toBe(cause)
    expect(document.querySelector('.cancel-error')).not.toBeInTheDocument()
    if (hint) expect(screen.getByPlaceholderText('Ask anything')).toHaveAccessibleDescription(hint)
    else expect(screen.getByPlaceholderText('Ask anything')).not.toHaveAccessibleDescription()
    expect(screen.queryByText('Muniment cannot reach its background service.')).not.toBeInTheDocument()
  })

  it.each([false, true])('keeps a rejected service call out of the composer when local mode is %s', async (local) => {
    localModeStatus = local
    const submission = deferred()
    signedIn([], submission.promise)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    submission.reject('Muniment cannot reach its background service.')
    const error = await within(document.querySelector('.thread')).findByText('Muniment cannot reach its background service.')
    expect(within(error).getByRole('button', { name: 'Try again' })).toBeEnabled()
    expect(document.querySelector('.cancel-error')).not.toBeInTheDocument()
    if (local) expect(screen.getByPlaceholderText('Ask anything')).not.toHaveAccessibleDescription()
    else expect(screen.getByPlaceholderText('Ask anything'))
      .toHaveAccessibleDescription('Routing is automatic. Every reply carries its receipt.')
  })

  it('shows Pi acquisition before the first reply event', async () => {
    signedIn([], { runId: 'run-9', attachments: [] })
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    const progress = 'Reply setup has started. Please wait.'
    chatListener({ payload: { runId: 'run-9', phase: 'acquiring-pi', text: '', toolActivity: [] } })
    await waitFor(() => expect(document.querySelector('.response')).toHaveTextContent(progress))
    expect(screen.getByTestId('run-announcement')).toHaveTextContent(progress)
    expect(screen.queryByText('Reply failed.')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Stop' })).toBeEnabled()
    chatListener({ payload: { runId: 'run-9', phase: 'streaming', text: 'A reply', toolActivity: [] } })
    await waitFor(() => expect(document.querySelector('.response')).toHaveTextContent('A reply'))
    expect(document.querySelector('.response')).not.toHaveTextContent(progress)
  })

  it.each([false, true])('shows acquisition failure when the submit response is pending: %s', async (pending) => {
    localModeStatus = true
    const submission = deferred()
    const result = { runId: 'run-acquisition', attachments: [] }
    signedIn([], pending ? submission.promise : result)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    const acquiring = { runId: result.runId, phase: 'acquiring-pi', text: '', toolActivity: [] }
    chatListener({ payload: acquiring })
    if (!pending) {
      await waitFor(() => expect(document.querySelector('.response')).toHaveTextContent('Reply setup has started. Please wait.'))
      expect(screen.getByRole('button', { name: 'Stop' })).toBeEnabled()
    }
    const reason = 'Reply setup failed. Check your connection and storage, then retry.'
    chatListener({ payload: { ...acquiring, phase: 'failed', failureReason: reason } })
    if (pending) submission.resolve(result)
    const error = await within(document.querySelector('.thread')).findByText(reason)
    expect(error).toHaveClass('run-error')
    expect(screen.getByTestId('run-announcement')).toHaveTextContent(reason)
    expect(screen.queryByRole('button', { name: 'Stop' })).not.toBeInTheDocument()
    expect(screen.queryByText('Reply setup has started. Please wait.')).not.toBeInTheDocument()
    expect(within(error).getByRole('button', { name: 'Try again' })).toBeEnabled()
    await fireEvent.input(composer, { target: { value: 'Another question' } })
    expect(screen.getByRole('button', { name: 'Send' })).not.toHaveAttribute('aria-disabled')
  })

  it('announces a permission pause during a streamed run', async () => {
    signedIn([], { runId: 'run-9', attachments: [] })
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    const region = screen.getByTestId('run-announcement')
    await waitFor(() => expect(region).toHaveTextContent('Generating a reply.'))
    const drain = watch(region)

    chatListener({ payload: { runId: 'run-9', phase: 'streaming', text: 'A', receipt: null, toolActivity: [] } })
    await waitFor(() => expect(document.querySelector('.response p')).toHaveTextContent('A'))
    expect(drain()).toEqual([])

    chatListener({ payload: {
      runId: 'run-9',
      phase: 'pending-permission',
      text: 'A routed',
      receipt: null,
      toolActivity: [],
      pendingPermission: { gateId: 'gate-9', kind: 'confirm', title: 'Run a command' },
    } })
    await waitFor(() => expect(region).toHaveTextContent('Waiting for your decision.'))
    expect(drain().length).toBeGreaterThan(0)

    for (const [phase, text] of [['streaming', 'A routed answer']]) {
      chatListener({ payload: { runId: 'run-9', phase, text, receipt: null, toolActivity: [] } })
      await waitFor(() => expect(document.querySelector('.response p')).toHaveTextContent(text))
    }
    expect(drain().length).toBeGreaterThan(0)
    expect(region).toHaveTextContent('Generating a reply.')

    chatListener({ payload: { runId: 'run-9', phase: 'complete', text: 'A routed answer', receipt: {}, toolActivity: [] } })

    await waitFor(() => expect(region).toHaveTextContent('Reply complete. A routed answer'))
    expect(drain()).toHaveLength(1)
    expect(screen.getByText('A routed answer')).toBeInTheDocument()
    expect(document.querySelector('.thread').textContent).not.toContain('Reply complete.')
  })

  it('drops a settled announcement before a restored thread is mounted', async () => {
    let authed = true
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: authed, subject: authed ? 'user-a' : null }
      if (command === 'auth_sign_in') { authed = true; return { signed_in: true, subject: 'user-a' } }
      if (command === 'auth_sign_out') { authed = false; return { signed_in: false, subject: null } }
      if (command === 'chat_thread_open') return restored
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return { runId: 'run-11', attachments: [] }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    chatListener({ payload: { runId: 'run-11', phase: 'complete', text: 'A routed answer', receipt: {}, toolActivity: [] } })
    await waitFor(() => expect(screen.getByTestId('run-announcement')).toHaveTextContent('Reply complete.'))

    await fireEvent.click(screen.getByRole('button', { name: /Alice/i }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Sign out' }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }))

    expect(await screen.findByText('Restored answer')).toBeInTheDocument()
    expect(screen.getByTestId('run-announcement').textContent).toBe('')
  })

  it('announces the completion of a reply that was restored mid-stream', async () => {
    signedIn([{ runId: 'run-live', phase: 'streaming', text: 'Half an', prompt: 'A question', receipt: null, toolActivity: [] }])
    expect(await screen.findByText('Half an')).toBeInTheDocument()
    const region = screen.getByTestId('run-announcement')
    await waitFor(() => expect(region).toHaveTextContent('Generating a reply.'))
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))
    const drain = watch(region)

    chatListener({ payload: { runId: 'run-live', phase: 'complete', text: 'Half an answer', receipt: {}, toolActivity: [] } })

    await waitFor(() => expect(region).toHaveTextContent('Reply complete. Half an answer'))
    expect(drain()).toHaveLength(1)
  })

  it('rejoins the run again after a sign-out and a sign-in', async () => {
    let authed = true
    let text = 'Half an'
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: authed, subject: authed ? 'user-a' : null }
      if (command === 'auth_sign_in') { authed = true; return { signed_in: true, subject: 'user-a' } }
      if (command === 'auth_sign_out') { authed = false; return { signed_in: false, subject: null } }
      if (command === 'chat_thread_open') return [{ runId: 'run-live', phase: 'streaming', text, prompt: 'A question', receipt: null, toolActivity: [] }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByRole('button', { name: 'Stop' })).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: /Alice/i }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Sign out' }))
    // The run kept streaming while the window was signed out.
    text = 'Half an answer'
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }))

    expect(await screen.findByText('Half an answer')).toBeInTheDocument()
    expect(await screen.findByRole('button', { name: 'Stop' })).toBeInTheDocument()
  })

  it('offers a rejoined reply the controls of a reply this desktop started', async () => {
    signedIn([{ runId: 'run-live', phase: 'streaming', text: 'Half an', prompt: 'A question', receipt: null, toolActivity: [] }])

    const stop = await screen.findByRole('button', { name: 'Stop' })
    expect(stop).toHaveClass('stop', 'composer-action')
    expect(stop).not.toHaveAttribute('title')
    expect(stop.querySelector('[data-icon="square"]')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Send' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Queue follow-up' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add files' })).not.toBeInTheDocument()
  })

  it('stays silent when a stray event lands on an already settled restored run', async () => {
    signedIn(restored)
    expect(await screen.findByText('Restored answer')).toBeInTheDocument()
    const region = screen.getByTestId('run-announcement')
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))
    const drain = watch(region)

    chatListener({ payload: { runId: 'run-old', phase: 'complete', text: 'Restored answer', receipt: {}, toolActivity: [{ effectId: 'tool-1', displayName: 'Read file', status: 'completed' }] } })

    // The event applies without a card: the thread draws no tool activity.
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(screen.queryByText('Read file')).not.toBeInTheDocument()
    expect(drain()).toEqual([])
    expect(region.textContent).toBe('')
  })

  it('announces a terminal outcome once for a run that never streamed', async () => {
    signedIn([], { runId: 'run-10', attachments: [] })
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    const region = screen.getByTestId('run-announcement')
    await waitFor(() => expect(region).toHaveTextContent('Generating a reply.'))
    const drain = watch(region)

    chatListener({ payload: { runId: 'run-10', type: 'failed' } })

    await waitFor(() => expect(region).toHaveTextContent('Reply failed.'))
    expect(drain()).toHaveLength(1)
  })

  it('records a stopped live reply and retries its prompt without a second live region', async () => {
    signedIn([], { runId: 'run-stopped', attachments: [] })
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Explain the record' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    chatListener({ payload: { runId: 'run-stopped', type: 'text-delta', text: 'A partial reply' } })

    await fireEvent.click(await screen.findByRole('button', { name: 'Stop' }))
    expect(invoke).toHaveBeenCalledWith('chat_cancel', { runId: 'run-stopped' })
    chatListener({ payload: { runId: 'run-stopped', type: 'cancelled' } })

    await waitFor(() => expect(document.querySelector('.run-error')).toHaveTextContent('Reply stopped.'))
    const record = document.querySelector('.run-error')
    expect(record).toHaveClass('run-error')
    expect(record).not.toHaveAttribute('role')
    expect(record).not.toHaveAttribute('aria-live')
    expect(screen.getByTestId('run-announcement')).toHaveTextContent('Reply stopped.')
    expect(document.querySelectorAll('.thread-shell [aria-live]')).toHaveLength(1)

    await fireEvent.click(within(record).getByRole('button', { name: 'Try again' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    expect(invoke).toHaveBeenLastCalledWith('chat_submit', { prompt: 'Explain the record', files: [] })
  })

  it('records a restored stopped reply without a retry when its prompt is absent', async () => {
    signedIn([{
      runId: 'run-restored-stopped', phase: 'cancelled', text: 'A restored partial reply',
      receipt: null, toolActivity: [],
    }])

    const record = await screen.findByText('Reply stopped.')
    expect(record).toHaveClass('run-error')
    expect(within(record).queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument()
    expect(screen.getByText('A restored partial reply')).toBeInTheDocument()
    expect(screen.getByTestId('run-announcement')).toHaveTextContent('')
  })
})

describe('tool activity', () => {
  it('draws no card for tool activity: the receipt tallies the calls instead', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-tools', phase: 'complete', text: 'I used tools.', prompt: 'Do work',
        receipt: { model: 'glm-5.2', time: '6.2s', tools: [{ name: 'bash', calls: 1, failed: 0 }, { name: 'grep', calls: 4, failed: 1 }] },
        toolActivity: [
          { effectId: 'tool-1', displayName: 'Search files', status: 'completed' },
          { effectId: 'tool-2', displayName: 'Read file', status: 'failed' },
        ],
      }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByText('I used tools.')).toBeInTheDocument()
    expect(screen.queryByText('Search files')).not.toBeInTheDocument()
    expect(screen.queryByRole('group', { name: /tool activity/i })).not.toBeInTheDocument()
    expect(document.querySelector('.tool-row, .tool-dot')).toBeNull()

    await fireEvent.click(screen.getByRole('button', { name: /^Expand receipt:/ }))
    expect(document.querySelector('.receipt-record').textContent).toBe('Toolsbash 1, grep 4 (1 failed)')
  })
})

describe('message action row', () => {
  const reply = (overrides = {}) => ({
    runId: 'run-copy', phase: 'complete', text: 'A routed answer', prompt: 'A question',
    receipt: {}, toolActivity: [], ...overrides,
  })

  function restore(history = [reply()]) {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return history
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return { runId: 'run-live', attachments: [] }
      throw new Error(`unexpected command: ${command}`)
    })
    return render(App)
  }

  function clipboard(writeText) {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    return writeText
  }

  const failureRecord = `Clipboard unavailable. Select the reply and press ${navigator.platform.startsWith('Mac') ? '⌘' : 'Ctrl '}C to copy it.`

  afterEach(() => { delete navigator.clipboard })

  // The row is revealed by CSS alone, so a keyboard user never depends on a pointer:
  // this test dispatches focus and the click that Enter on a focused button produces,
  // and no hover event at all.
  it('reaches and activates Copy without any pointer hover', async () => {
    const writeText = clipboard(vi.fn().mockResolvedValue(undefined))
    restore()

    const copy = await screen.findByRole('button', { name: 'Copy' })
    expect(copy.closest('.message-actions')).toBeInTheDocument()
    expect(copy).not.toHaveAttribute('tabindex')
    expect(copy).not.toHaveAttribute('hidden')
    copy.focus()
    expect(copy).toHaveFocus()
    await fireEvent.click(copy)

    await waitFor(() => expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument())
    expect(writeText).toHaveBeenCalledWith('A routed answer')
    expect(screen.getByTestId('message-action-announcement')).toHaveTextContent('Reply copied to the clipboard.')
  })

  it('reverts the confirmation after about two seconds', async () => {
    clipboard(vi.fn().mockResolvedValue(undefined))
    restore()
    vi.useFakeTimers({ shouldAdvanceTime: true })

    await fireEvent.click(await screen.findByRole('button', { name: 'Copy' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument())

    await vi.advanceTimersByTimeAsync(1500)
    expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument()

    await vi.advanceTimersByTimeAsync(600)
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument()
    expect(screen.getByTestId('message-action-announcement').textContent).toBe('')
  })

  it('announces a repeated copy of the same reply again', async () => {
    clipboard(vi.fn().mockResolvedValue(undefined))
    restore()
    const region = await screen.findByTestId('message-action-announcement')
    const changes = []
    new MutationObserver((records) => changes.push(...records)).observe(region, { childList: true, characterData: true, subtree: true })

    await fireEvent.click(await screen.findByRole('button', { name: 'Copy' }))
    await waitFor(() => expect(region).toHaveTextContent('Reply copied to the clipboard.'))
    await fireEvent.click(screen.getByRole('button', { name: 'Copied' }))

    await waitFor(() => expect(changes.length).toBeGreaterThanOrEqual(3))
    expect(region).toHaveTextContent('Reply copied to the clipboard.')
  })

  it('leaves an inline mono record when the clipboard refuses and returns the button to Copy', async () => {
    clipboard(vi.fn().mockRejectedValue(new Error('denied')))
    restore()

    await fireEvent.click(await screen.findByRole('button', { name: 'Copy' }))

    const record = await waitFor(() => within(document.querySelector('.response')).getByText(failureRecord))
    expect(record).toHaveClass('run-error')
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Copied' })).not.toBeInTheDocument()
    expect(screen.getByTestId('message-action-announcement')).toHaveTextContent(failureRecord)
  })

  it('records a failure when the webview exposes no clipboard at all', async () => {
    restore()

    await fireEvent.click(await screen.findByRole('button', { name: 'Copy' }))

    await waitFor(() => expect(within(document.querySelector('.response')).getByText(failureRecord)).toBeInTheDocument())
  })

  it('copies the reply the button belongs to and confirms on that one only', async () => {
    const writeText = clipboard(vi.fn().mockResolvedValue(undefined))
    restore([reply(), reply({ runId: 'run-copy-2', text: 'A second answer' })])

    const responses = await waitFor(() => {
      const found = document.querySelectorAll('.response')
      expect(found).toHaveLength(2)
      return found
    })
    await fireEvent.click(within(responses[1]).getByRole('button', { name: 'Copy' }))

    await waitFor(() => expect(within(responses[1]).getByRole('button', { name: 'Copied' })).toBeInTheDocument())
    expect(writeText).toHaveBeenCalledWith('A second answer')
    expect(within(responses[0]).getByRole('button', { name: 'Copy' })).toBeInTheDocument()
  })

  it('never overwrites the run-phase announcement with a copy confirmation', async () => {
    clipboard(vi.fn().mockResolvedValue(undefined))
    restore([])
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'A question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    chatListener({ payload: { runId: 'run-live', phase: 'complete', text: 'A routed answer', receipt: {}, toolActivity: [] } })
    const runRegion = screen.getByTestId('run-announcement')
    await waitFor(() => expect(runRegion).toHaveTextContent('Reply complete. A routed answer'))

    await fireEvent.click(await screen.findByRole('button', { name: 'Copy' }))

    await waitFor(() => expect(screen.getByTestId('message-action-announcement')).toHaveTextContent('Reply copied to the clipboard.'))
    expect(runRegion).toHaveTextContent('Reply complete. A routed answer')
    expect(document.querySelectorAll('.thread-shell [aria-live]')).toHaveLength(1)
  })

  it('shows the local send time and copies the user message with an icon-only button', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    clipboard(writeText)
    const sentAt = '2026-09-19T01:17:00Z'
    restore([reply({ sentAt })])
    const button = await screen.findByRole('button', { name: 'Copy message' })
    const turn = button.closest('.user-turn')
    expect(turn.querySelector('time')).toHaveAttribute('datetime', sentAt)
    expect(turn.querySelector('time')).toHaveTextContent(new Date(sentAt).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }))
    expect(button.textContent).toBe('')
    await fireEvent.click(button)
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(turn.querySelector('.user-message p').textContent))
    expect(await screen.findByRole('button', { name: 'Copied message' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy' }).textContent).toBe('')
  })

  it('offers copy alone: no fork, share, or retry on a settled reply', async () => {
    restore()

    await screen.findByRole('button', { name: 'Copy' })
    expect(document.querySelectorAll('.response .message-actions button')).toHaveLength(1)
    for (const name of [/fork/i, /share/i, /retry/i, /try again/i]) {
      expect(screen.queryByRole('button', { name })).not.toBeInTheDocument()
    }
  })

  it('withholds the row until the run settles', async () => {
    restore([reply({ phase: 'streaming', receipt: null })])

    expect(await screen.findByText('A routed answer')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Copy' })).not.toBeInTheDocument()
  })

  it('copies the exact plain text when a reply settles without content', async () => {
    const writeText = clipboard(vi.fn().mockResolvedValue(undefined))
    restore([reply({ text: '' })])

    await fireEvent.click(await screen.findByRole('button', { name: 'Copy' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(''))
  })
})

describe('provenance line', () => {
  function restore(receipt, recalls = []) {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{ runId: 'run-receipt', phase: 'complete', text: 'A routed answer', prompt: 'A question', receipt, recalls, toolActivity: [] }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    return render(App)
  }

  it('renders route → model and the clock time with signal on the route segment', async () => {
    restore({ route: 'analysis/high', model: 'glm-5.2', cost: '$0.0089', time: '6.2s' })

    const line = await screen.findByRole('button', { name: 'Expand receipt: Routed via analysis/high to model glm 5.2, 6.2s' })
    expect(line.textContent).toBe('analysis/high → glm 5.2')
    expect(line.closest('.receipt-line').querySelector('.response-meta .receipt-time [data-icon="clock"]')).toBeInTheDocument()
    expect(within(line).getByText('analysis/high')).toHaveClass('route-segment')
    expect(line.querySelectorAll('.route-segment')).toHaveLength(1)
  })

  it('paints nothing green when the receipt records no route', async () => {
    restore({ model: 'glm-5.2', cost: '$0.0089', time: '6.2s' })

    const line = await screen.findByRole('button', { name: 'Expand receipt: Model glm 5.2, 6.2s' })
    expect(line.textContent).toBe('glm 5.2')
    expect(line.querySelector('.route-segment')).toBeNull()
  })

  it('renders elapsed time alone for a local receipt as a plain line with a clock and no expand control', async () => {
    restore({ time: '6.2s' })

    const line = await screen.findByLabelText('Receipt: 6.2s', { selector: 'p.provenance' })
    expect(line.closest('.receipt-line').querySelector('.response-meta')).toHaveTextContent('6.2s')
    expect(line.closest('.receipt-line').querySelector('.response-meta [data-icon="clock"]')).toBeInTheDocument()
    expect(line.querySelector('.receipt-marker')).toBeNull()
    expect(line.querySelector('.route-segment')).toBeNull()
    expect(screen.queryByRole('button', { name: /receipt/i })).not.toBeInTheDocument()
    expect(screen.queryByText('Receipt unavailable')).not.toBeInTheDocument()
  })

  it('renders a route-only receipt without a dangling arrow', async () => {
    restore({ route: 'analysis/high' })

    const line = await waitFor(() => {
      const plain = document.querySelector('p.provenance')
      expect(plain).not.toBeNull()
      return plain
    })
    expect(line).toHaveAttribute('aria-label', 'Receipt: Routed via analysis/high')
    expect(line.querySelector('.route-segment')).toHaveTextContent('analysis/high')
    // No time, so no clock: the clock belongs to the time segment alone.
    expect(line.querySelector('[data-icon="clock"]')).toBeNull()
  })

  it('expands to the receipt record and back', async () => {
    restore({ route: 'analysis/high', model: 'glm-5.2', cost: '$0.0089', time: '6.2s', capabilities: [{ name: 'search', version: '2' }] })

    const line = await screen.findByRole('button', { name: /^Expand receipt:/ })
    const marker = line.querySelector('.receipt-marker')
    expect(line.textContent).toBe('analysis/high → glm 5.2')
    expect(marker).toHaveAttribute('aria-hidden', 'true')
    expect(marker).not.toHaveClass('expanded')
    await fireEvent.click(line)

    const record = document.querySelector('.receipt-record')
    // The rows carry what the line does not: nothing appears twice.
    expect(record.textContent).toBe('Cost$0.0089Capabilitysearch@2')
    expect(record.querySelectorAll('.route-value')).toHaveLength(0)
    expect(await screen.findByRole('button', { name: 'Collapse receipt: Routed via analysis/high to model glm 5.2, 6.2s' })).toBe(line)
    expect(marker).toHaveClass('expanded')

    await fireEvent.click(line)
    await waitFor(() => expect(document.querySelector('.receipt-record')).toBeNull())
    expect(marker).not.toHaveClass('expanded')
  })

  it('names each recall and puts each recalled file on its own line', async () => {
    restore(
      { route: 'analysis/high', model: 'glm-5.2', cost: '$0.0089', time: '6.2s' },
      [
        { query: 'lease', files: ['Documents/Muniment/lease.pdf', 'Documents/Muniment/notes.md'] },
        { query: 'missing clause', files: [] },
      ],
    )

    await fireEvent.click(await screen.findByRole('button', { name: /^Expand receipt:/ }))

    const record = document.querySelector('.receipt-record')
    expect(record.textContent).toBe('Cost$0.0089Memorylease, 2 filesDocuments/Muniment/lease.pdfDocuments/Muniment/notes.mdMemorymissing clause, 0 files')
    expect([...record.querySelectorAll('.recall-file')].map((file) => file.textContent)).toEqual([
      'Documents/Muniment/lease.pdf',
      'Documents/Muniment/notes.md',
    ])
  })

  it('renders a provenance line when a reply has an empty receipt', async () => {
    restore({})

    expect(await screen.findByText('A routed answer')).toBeInTheDocument()
    const line = screen.getByText('Receipt unavailable')
    expect(line).toHaveClass('provenance')
    expect(line.querySelector('.receipt-marker')).toBeNull()
  })

  it('renders a provenance line when a reply has no receipt', async () => {
    restore(null)

    expect(await screen.findByText('A routed answer')).toBeInTheDocument()
    const line = screen.getByText('Receipt unavailable')
    expect(line).toHaveClass('provenance')
    expect(line.querySelector('.receipt-marker')).toBeNull()
  })
})

describe('active run composer queue', () => {
  beforeEach(() => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
    await screen.findByRole('button', { name: 'Stop' })
    // The workspace remounts once the desktop client status lands, so the
    // composer that takes Enter is the one on screen now.
    return screen.getByPlaceholderText('Ask anything')
  }

  function expectQueuePayload(payload) {
    const call = invoke.mock.calls.find(([command]) => command === 'chat_queue')
    // These are the camelCased flattened chat_queue parameters in src-tauri/src/chat.rs;
    // delivery spellings come from ChatDelivery's rename_all = "camelCase".
    expect(call?.[1]).toEqual(payload)
  }

  it('steers the active reply from Enter with the exact Rust command payload', async () => {
    const composer = await startRun()
    await fireEvent.input(composer, { target: { value: 'Focus on the risks' } })
    // In flight the band holds the stop square alone: no send control and no delivery mode.
    expect(screen.queryByRole('button', { name: 'Send' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Queue follow-up' })).not.toBeInTheDocument()
    expect(document.getElementById('composer-hint')).toBeNull()
    await fireEvent.keyDown(composer, { key: 'Enter' })

    expectQueuePayload({ runId: 'run-7', delivery: 'steer', message: 'Focus on the risks' })
  })

  it('keeps focus on the same action control as it turns from send to stop', async () => {
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await waitFor(() => expect(composer).toBeEnabled())
    await fireEvent.input(composer, { target: { value: 'Initial prompt' } })
    const send = screen.getByRole('button', { name: 'Send' })
    send.focus()

    await fireEvent.click(send)

    expect(document.activeElement).toBe(send)
    expect(screen.getByRole('button', { name: 'Stop' })).toBe(send)
    expect(send).toHaveClass('stop')
    expect(send).not.toHaveClass('primary')
  })

  it('shows a queue rejection and preserves the draft', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return { runId: 'run-7' }
      if (command === 'chat_queue') throw 'Could not queue this message'
      throw new Error(`unexpected command: ${command}`)
    })
    const composer = await startRun()
    await fireEvent.input(composer, { target: { value: 'Keep this draft' } })
    await fireEvent.keyDown(composer, { key: 'Enter' })

    expect(await screen.findByRole('alert')).toHaveTextContent('Could not queue this message')
    expect(composer).toHaveValue('Keep this draft')
  })
})

describe('composer auto-grow', () => {
  // jsdom lays nothing out, so the §4 clamp has nothing to measure. Give the
  // textarea a real row height and a content height that tracks its value.
  const row = 20
  const resting = `${2 * row}px`
  const cap = `${10 * row}px`
  const lines = (count) => Array.from({ length: count }, (_, index) => `line ${index + 1}`).join('\n')
  // A narrower composer soft-wraps the same draft onto more rendered lines.
  // The resize test models that behavior by replacing this.
  let rendered

  beforeEach(() => {
    rendered = (value) => value.split('\n').length
    Object.defineProperty(HTMLTextAreaElement.prototype, 'scrollHeight', {
      configurable: true,
      get() { return rendered(this.value) * row },
    })
    const computed = window.getComputedStyle.bind(window)
    vi.spyOn(window, 'getComputedStyle').mockImplementation((element, pseudo) => element instanceof HTMLTextAreaElement
      ? { lineHeight: `${row}px`, paddingTop: '0px', paddingBottom: '0px' }
      : computed(element, pseudo))
  })

  afterEach(() => {
    delete HTMLTextAreaElement.prototype.scrollHeight
    delete globalThis.ResizeObserver
  })

  it('grows to the ten-line cap, then scrolls instead of growing further', async () => {
    render(App)
    const composer = await findWorkspaceComposer()
    expect(composer.style.height).toBe(resting)

    await fireEvent.input(composer, { target: { value: lines(5) } })
    expect(composer.style.height).toBe(`${5 * row}px`)
    expect(composer.style.overflowY).toBe('hidden')

    await fireEvent.input(composer, { target: { value: lines(10) } })
    expect(composer.style.height).toBe(cap)

    await fireEvent.input(composer, { target: { value: lines(40) } })
    expect(composer.style.height).toBe(cap)
    expect(composer.style.overflowY).toBe('auto')
  })

  it('shrinks as text is deleted and rests at two rows once the draft clears', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return { runId: 'run-9', attachments: [] }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: lines(8) } })
    expect(composer.style.height).toBe(`${8 * row}px`)

    await fireEvent.input(composer, { target: { value: lines(3) } })
    expect(composer.style.height).toBe(`${3 * row}px`)

    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    await waitFor(() => expect(composer).toHaveValue(''))
    expect(composer.style.height).toBe(resting)
  })

  it('resizes for programmatic draft changes, not only typed input', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: lines(3) } })
    expect(composer.style.height).toBe(`${3 * row}px`)

    globalShortcutHandler({ state: 'Pressed' })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_start'))
    dictationListener({ payload: { type: 'transcript', text: `\n${lines(4)}` } })
    await waitFor(() => expect(composer.style.height).toBe(`${7 * row}px`))

    // Esc cancels the capture and restores the snapshot; the box follows it back.
    await fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => expect(composer.style.height).toBe(`${3 * row}px`))
  })

  it('reveals streamed transcript chunks past the cap while the composer is focused', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    composer.focus()

    composer.scrollTop = 37
    await fireEvent.input(composer, { target: { value: lines(12) } })
    expect(composer).toHaveFocus()
    expect(composer.scrollTop).toBe(37)

    globalShortcutHandler({ state: 'Pressed' })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_start'))
    dictationListener({ payload: { type: 'transcript', text: `\n${lines(4)}` } })
    await waitFor(() => expect(composer.scrollTop).toBe(composer.scrollHeight))

    composer.scrollTop = 41
    dictationListener({ payload: { type: 'transcript', text: `\n${lines(3)}` } })
    await waitFor(() => expect(composer.scrollTop).toBe(composer.scrollHeight))
    expect(composer).toHaveFocus()
  })

  it('returns to the resting height through the Try again retry', async () => {
    const submitted = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{
        runId: 'run-interrupted', phase: 'interrupted', text: 'Partial answer',
        prompt: lines(4), receipt: null, toolActivity: [], resumable: false,
      }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return submitted.promise
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await findWorkspaceComposer()
    expect(composer.style.height).toBe(resting)

    // Retry writes the four-line prompt into the draft with no input event,
    // then send() clears it once the submission lands.
    await fireEvent.click(await screen.findByRole('button', { name: 'Try again' }))
    await waitFor(() => expect(composer.style.height).toBe(`${4 * row}px`))
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    submitted.resolve({ runId: 'run-11', attachments: [] })
    await waitFor(() => expect(composer.style.height).toBe(resting))
  })

  it('re-measures when the composer itself resizes, not only the window', async () => {
    const observers = []
    globalThis.ResizeObserver = class {
      constructor(callback) { this.callback = callback; this.disconnected = false; observers.push(this) }
      observe(target) { this.target = target }
      disconnect() { this.disconnected = true }
    }
    render(App)
    const composer = await findWorkspaceComposer()
    await fireEvent.input(composer, { target: { value: lines(6) } })
    expect(composer.style.height).toBe(`${6 * row}px`)

    // Opening the artifact rail narrows the composer without touching the
    // window: the same draft rewraps onto eight lines. Left unmeasured the box
    // stays six rows tall with overflow hidden, so two lines of the user's own
    // draft would be invisible and unreachable.
    rendered = (value) => value.split('\n').length + 2
    // The action row, never the input: observing a box this callback resizes
    // makes the browser report an undelivered ResizeObserver loop on every drag.
    const rowObserver = observers.find((observer) => observer.target.classList.contains('composer-row'))
    expect(rowObserver).toBeDefined()
    rowObserver.callback()
    expect(composer.style.height).toBe(`${8 * row}px`)
    expect(composer.style.overflowY).toBe('hidden')

    // The composer box's own height reaches the thread panel as --composer-height,
    // so the transcript's bottom padding and the fade above the composer follow it.
    const boxObserver = observers.find((observer) => observer.target.classList.contains('composer'))
    expect(boxObserver).toBeDefined()
    Object.defineProperty(boxObserver.target, 'offsetHeight', { value: 212, configurable: true })
    boxObserver.callback()
    await tick()
    expect(document.querySelector('.thread-panel').style.getPropertyValue('--composer-height')).toBe('212px')

    cleanup()
    expect(observers.every((observer) => observer.disconnected)).toBe(true)
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
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') {
        accessCalls += 1
        // The sidebar reads the snapshot once per signed-in session.
        if (accessCalls <= 1) return snapshot()
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
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Sign out' }))
    await screen.findByRole('button', { name: 'Sign in' })

    await fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))
    const newProfile = await screen.findByRole('button', { name: /user-b/i })
    expect(newProfile).toHaveTextContent('Access unavailable')
    expect(screen.queryByText('Alice')).not.toBeInTheDocument()
    expect(screen.queryByText('Acme · owner')).not.toBeInTheDocument()

    // The sidebar retries a failed snapshot, so the count is a floor.
    expect(accessCalls).toBeGreaterThanOrEqual(2)
    rejectNewSessionAccess(new Error('offline'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.queryByText('Alice')).not.toBeInTheDocument()
    expect(screen.queryByText('Acme · owner')).not.toBeInTheDocument()
  })

  it('keeps fetch states local, retries, and expands duplicate grants independently', async () => {
    let rejectOpen
    const pendingOpen = new Promise((_, reject) => { rejectOpen = reject })
    let accessCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') {
        accessCalls += 1
        if (accessCalls === 1) return snapshot()
        if (accessCalls === 2) return pendingOpen
        return snapshot([
          grant({ principal_id: null }),
          grant({ id: 'grt_second', principal_type: 'group', principal_id: 'members' }),
        ])
      }
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return []
      if (command === 'attach_listener_status') return { started: true, failure: null }
      throw new Error(`unexpected command: ${command}`)
    })

    render(App)
    const profile = await screen.findByRole('button', { name: /Alice/i })
    const dialog = await openSettings('Account')

    expect(within(dialog).getByText('Checking your current access…')).toBeInTheDocument()
    expect(profile).toHaveTextContent('Acme · owner')
    expect(screen.getByText(/Ask a question or request a file/)).toBeInTheDocument()

    rejectOpen(new Error('offline'))
    const retry = await within(dialog).findByRole('button', { name: 'Try again' })
    expect(screen.getByText(/Ask a question or request a file/)).toBeInTheDocument()
    await fireEvent.click(retry)
    expect(accessCalls).toBe(3)

    const toggles = await within(dialog).findAllByRole('button', { name: 'allow use · gpt' })
    expect(toggles).toHaveLength(2)
    expect(toggles[0]).toHaveAttribute('aria-expanded', 'false')
    expect(toggles[1]).toHaveAttribute('aria-expanded', 'false')
    await fireEvent.click(toggles[0])
    expect(toggles[0]).toHaveAttribute('aria-expanded', 'true')
    expect(toggles[1]).toHaveAttribute('aria-expanded', 'false')
    expect(within(dialog).getByText('org · Not specified')).toBeInTheDocument()
    expect(within(dialog).getByText('The grant has no expiration.')).toBeInTheDocument()
    expect(within(dialog).queryByText('group · members')).not.toBeInTheDocument()

  })

  it('renders mixed device states in deterministic order without disturbing grants', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot([grant()])
      if (command === 'auth_devices') return [
        device('revoked-newest', { platform: 'ios', revoked_at: '2026-06-01T00:00:00Z', last_active_at: '2026-07-01T00:00:00Z' }),
        device('active-old', { platform: 'android', last_active_at: '2026-05-01T00:00:00Z' }),
        device('current-new', { current: true, last_active_at: '2026-06-01T00:00:00Z' }),
      ]
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const dialog = await openSettings('Account')
    // The settings nav is a list too, so read the device list itself.
    await within(dialog).findByText('Android')
    const rows = [...dialog.querySelectorAll('.device-list li')]
    expect(rows.map((row) => row.textContent)).toEqual(expect.arrayContaining([
      expect.stringContaining('This device'), expect.stringContaining('Active'), expect.stringContaining('Revoked'),
    ]))
    expect(rows[0]).toHaveTextContent('DesktopThis deviceActive')
    expect(rows[1]).toHaveTextContent('AndroidActive')
    expect(rows[2]).toHaveTextContent('iOSRevoked')
    expect(rows[2]).toHaveClass('revoked')
    const activeIdentifier = within(rows[1]).getByText('Android')
    const revokedIdentifier = within(rows[2]).getByText('iOS')
    const revokedRule = accountSettingsSource.match(/\.revoked \.device-heading strong\s*\{([^}]*)\}/)?.[1]
    expect(revokedIdentifier.tagName).toBe('STRONG')
    expect(activeIdentifier.tagName).toBe('STRONG')
    expect(revokedRule).toMatch(/color:\s*var\(--oxide\)/)
    expect(revokedRule).toMatch(/text-decoration:\s*line-through/)
    expect(revokedRule).toMatch(/font-weight:\s*400/)
    expect(within(rows[2]).getByText('Revoked')).toBeVisible()
    expect(within(dialog).getByRole('button', { name: 'allow use · gpt' })).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('revoked-newest')
  })

  it('renders an empty device response', async () => {
    render(App)
    await openSettings('Account')
    expect(await screen.findByText('No devices found')).toBeInTheDocument()
  })

  it('applies and persists an accessible per-device appearance choice', async () => {
    render(App)
    await openSettings('Preferences')
    const appearance = within(screen.getByRole('group', { name: 'Mode' }))
    const system = appearance.getByRole('button', { name: 'System' })
    const dark = appearance.getByRole('button', { name: 'Dark' })

    expect(system).toHaveAttribute('aria-pressed', 'true')
    expect(dark).toHaveAttribute('aria-pressed', 'false')

    await fireEvent.click(dark)
    expect(document.documentElement.dataset.theme).toBe('vault')
    expect(document.documentElement.dataset.scheme).toBe('dark')
    expect(JSON.parse(localStorage.getItem('muniment.theme'))).toEqual({ mode: 'dark', light: 'paper', dark: 'vault' })
    expect(dark).toHaveAttribute('aria-pressed', 'true')

    await fireEvent.click(system)
    expect(document.documentElement).not.toHaveAttribute('data-theme')
    expect(document.documentElement).not.toHaveAttribute('data-scheme')
    expect(JSON.parse(localStorage.getItem('muniment.theme')).mode).toBe('system')
    expect(system).toHaveAttribute('aria-pressed', 'true')
  })

  it('orders the account sections and leaves sign out to the sidebar', async () => {
    render(App)
    const dialog = await openSettings('Account')
    const content = dialog.querySelector('.account-sections')

    expect(content).toBeInTheDocument()
    expect(within(dialog).queryByRole('button', { name: 'Sign out' })).not.toBeInTheDocument()
    expect([...content.querySelectorAll(':scope > section')].map((section) => section.getAttribute('aria-labelledby'))).toEqual([
      'retention-heading',
      'entitlements-heading',
      'devices-heading',
      'companions-heading',
      'voice-heading',
    ])
    expect(content.querySelector('.entitlements-section')).toHaveTextContent('Your admins set access.')
  })

  it('identifies the profile in the sidebar and keeps snapshot metadata in its section', async () => {
    render(App)
    const profile = await screen.findByRole('button', { name: /Alice/i })
    expect(profile).not.toHaveAttribute('title')
    expect(profile).toHaveTextContent('Alice')
    expect(profile).toHaveTextContent('Acme · owner')
    expect(profile).not.toHaveTextContent('Snapshot')

    const dialog = await openSettings('Account')
    const accessSection = dialog.querySelector('.entitlements-section')
    expect(within(dialog).getAllByRole('heading', { name: 'Your access' })).toHaveLength(1)
    expect(accessSection).toHaveTextContent('Snapshot v2')
    expect(dialog).not.toHaveTextContent('Your groups')
  })

  it('retries only a failed device request and keeps entitlement grants rendered', async () => {
    let deviceCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot([grant()])
      if (command === 'auth_devices') {
        deviceCalls += 1
        if (deviceCalls === 1) throw new Error('raw backend secret')
        return [device('recovered')]
      }
      if (command === 'attach_companions') return []
      if (command === 'attach_listener_status') return { started: true, failure: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const dialog = await openSettings('Account')
    expect(await within(dialog).findByText('Devices could not be loaded.')).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('raw backend secret')
    expect(within(dialog).getByRole('button', { name: 'allow use · gpt' })).toBeInTheDocument()
    const entitlementCalls = invoke.mock.calls.filter(([command]) => command === 'auth_entitlement_snapshot').length
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Try again' }))
    expect(await within(dialog).findByText('Active')).toBeInTheDocument()
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_entitlement_snapshot')).toHaveLength(entitlementCalls)
  })

  it('shows connected programs while loading and renders claimed records with missing approval times', async () => {
    const pendingCompanions = deferred()
    const longKind = 'command-line-program-with-a-name-that-does-not-fit'
    const longVersion = '2026.08.04-preview-with-a-version-that-does-not-fit'
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return pendingCompanions.promise
      if (command === 'attach_listener_status') return { started: true, failure: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const dialog = await openSettings('Account')
    expect(within(dialog).getByText('Loading connected programs…')).toBeInTheDocument()

    pendingCompanions.resolve([{ identity: 'client-1', claimed_kind: longKind, claimed_version: longVersion, approved_at: null }])
    const kind = await within(dialog).findByText(longKind)
    expect(kind).not.toHaveAttribute('title')
    expect(within(dialog).getByText('Claimed kind')).toBeInTheDocument()
    expect(within(dialog).getByText(`Claimed version: ${longVersion}`)).not.toHaveAttribute('title')
    expect(within(dialog).getByText('Approval time unavailable')).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('client-1')
  })

  it('renders an empty connected program response', async () => {
    render(App)
    await openSettings('Account')
    expect(await screen.findByText('No connected programs found')).toBeInTheDocument()
  })

  it('retries only a failed connected program request', async () => {
    let companionCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'project_list') return { projects: {}, threads: {} }
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') {
        companionCalls += 1
        if (companionCalls === 1) throw new Error('raw backend secret')
        return [{ identity: 'client-2', claimed_kind: 'ACP adapter', claimed_version: '2.0.0', approved_at: '2026-08-04T12:00:00Z' }]
      }
      if (command === 'attach_listener_status') return { started: true, failure: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await openSettings('Account')
    const section = screen.getByRole('heading', { name: 'Connected programs' }).closest('section')
    expect(await within(section).findByText('Connected programs could not be loaded.')).toBeInTheDocument()
    expect(section).not.toHaveTextContent('raw backend secret')
    await fireEvent.click(within(section).getByRole('button', { name: 'Try again' }))
    expect(await within(section).findByText('ACP adapter')).toBeInTheDocument()
    expect(companionCalls).toBe(2)
  })
})


describe('account display names', () => {
  it('saves a trimmed name inline and cancels without saving', async () => {
    const { default: ModelAccounts } = await import('./lib/ModelAccounts.svelte')
    const account = { id: 'name-test', family: 'xai', label: 'user@example.test', source: 'subscription', enabled: true, weight: 1, models: [], days: [], windows: [], requests: 0, input_tokens: 0, output_tokens: 0, errors: 0 }
    const settings = { accounts: [account], families: [], subscriptions: [] }
    const tauri = { invoke: vi.fn(async () => settings) }
    const view = render(ModelAccounts, { tauri, settings, family: 'xai' })
    await fireEvent.click(view.getByRole('button', { name: 'Rename user@example.test' }))
    await fireEvent.input(view.getByLabelText('Account name'), { target: { value: '  Work Grok  ' } })
    await fireEvent.submit(view.getByLabelText('Account name').closest('form'))
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('model_router_update_account', { id: 'name-test', label: 'Work Grok' }))
    await waitFor(() => expect(view.queryByLabelText('Account name')).toBeNull())
    await fireEvent.click(view.getByRole('button', { name: 'Rename user@example.test' }))
    await fireEvent.keyDown(view.getByLabelText('Account name'), { key: 'Escape' })
    expect(tauri.invoke).toHaveBeenCalledTimes(1)
    tauri.invoke.mockRejectedValue(new Error('Save failed'))
    await fireEvent.click(view.getByRole('button', { name: 'Rename user@example.test' }))
    await fireEvent.submit(view.getByLabelText('Account name').closest('form'))
    expect(await view.findByRole('alert')).toHaveTextContent('Save failed')
    expect(view.getByLabelText('Account name')).toBeInTheDocument()
  })
})

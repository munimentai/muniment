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
let requiredModelInvoke
let globalShortcutHandler
let registerGlobalShortcut
let unregisterGlobalShortcut
let registeredShortcuts

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: (...args) => registerGlobalShortcut(...args),
  unregister: (...args) => unregisterGlobalShortcut(...args),
}))

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

async function stopClickCapture(voice) {
  await fireEvent.click(voice)
  await fireEvent.click(voice)
}

function deferred() {
  let resolve
  const promise = new Promise((res) => { resolve = res })
  return { promise, resolve }
}

beforeAll(async () => {
  HTMLElement.prototype.scrollTo = vi.fn()
  window.__TAURI__ = {
    core: { invoke: (command, ...args) => command === 'home_status'
      ? Promise.resolve(homeStatus)
      : command === 'required_model_acquisition_status'
        ? requiredModelInvoke(...args)
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
  localStorage.clear()
  homeStatus = { configured: true, homePath: '/Documents/Muniment' }
  requiredModelInvoke = vi.fn().mockResolvedValue({ status: { state: 'installed' }, downloadedBytes: 100, totalBytes: 100, folderSetupAvailable: true, aiFeaturesAvailable: true, retryingInBackground: false })
  chatListener = undefined
  dictationListener = undefined
  eventUnlisten = vi.fn()
  pairingUnlisten = vi.fn()
  dragDropListener = undefined
  dragDropUnlisten = vi.fn()
  dialogResult = null
  globalShortcutHandler = undefined
  registeredShortcuts = new Set()
  registerGlobalShortcut = vi.fn(async (shortcut, handler) => {
    registeredShortcuts.add(shortcut)
    globalShortcutHandler = handler
  })
  unregisterGlobalShortcut = vi.fn(async (shortcut) => { registeredShortcuts.delete(shortcut) })
  invoke = vi.fn(async (command, payload) => {
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_history') return []
    if (command === 'chat_file_metadata') return { displayName: payload.path.split(/[\\/]/).pop(), byteLength: 1536 }
    if (command === 'auth_entitlement_snapshot') return snapshot()
    if (command === 'auth_devices') return []
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('artifact rail', () => {
  it('toggles from the titlebar button with accessible state and an honest empty landmark', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByRole('complementary', { name: 'Artifacts' })).not.toBeInTheDocument()

    await fireEvent.click(toggle)

    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(toggle).toHaveAccessibleName('Close artifact rail')
    const rail = screen.getByRole('complementary', { name: 'Artifacts' })
    expect(within(rail).getByText('No artifacts yet')).toBeInTheDocument()
    expect(within(rail).getByText('Artifacts created in this thread will appear here.')).toBeInTheDocument()

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

  it('does not toggle from an input or textarea', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    const composer = screen.getByPlaceholderText('Ask anything')
    const mac = navigator.platform.startsWith('Mac')
    await fireEvent.keyDown(composer, { key: 'j', metaKey: mac, ctrlKey: !mac })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')

    const input = document.createElement('input')
    document.body.append(input)
    await fireEvent.keyDown(input, { key: 'j', metaKey: mac, ctrlKey: !mac })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
  })
})

describe('Home onboarding', () => {
  it('shows active model progress and stops polling when local AI becomes ready', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    requiredModelInvoke
      .mockResolvedValueOnce({ status: { state: 'installing' }, downloadedBytes: 25, totalBytes: 100, folderSetupAvailable: true, aiFeaturesAvailable: false, retryingInBackground: true })
      .mockResolvedValueOnce({ status: { state: 'installed' }, downloadedBytes: 100, totalBytes: 100, folderSetupAvailable: true, aiFeaturesAvailable: true, retryingInBackground: false })
    render(App)
    const progress = await screen.findByRole('progressbar', { name: 'Required local AI model download' })
    expect(progress).toHaveAttribute('aria-valuemin', '0')
    expect(progress).toHaveAttribute('aria-valuemax', '100')
    expect(progress).toHaveAttribute('aria-valuenow', '25')
    expect(screen.getByText('25 B of 100 B')).toBeInTheDocument()
    await vi.advanceTimersByTimeAsync(1000)
    expect(await screen.findByText('Local proposal generation is ready.')).toBeInTheDocument()
    expect(screen.getByText('100 B of 100 B')).toBeInTheDocument()
    expect(screen.getByRole('progressbar', { name: 'Required local AI model download' })).toHaveAttribute('aria-valuenow', '100')
    await vi.advanceTimersByTimeAsync(2000)
    expect(requiredModelInvoke).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })

  it('shows redacted retry status without blocking fail-open onboarding actions', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    requiredModelInvoke.mockResolvedValue({ status: { state: 'failed', category: 'network', message: 'redacted' }, downloadedBytes: 40, totalBytes: 100, folderSetupAvailable: true, aiFeaturesAvailable: false, retryingInBackground: true })
    invoke.mockImplementation(async (command) => {
      if (command === 'onboarding_import_preview') return { entries: [{ name: 'profile.json', kind: 'json', byteSize: 2, excerpt: '{}', excerptTruncated: false }], totalByteSize: 2 }
      if (command === 'onboarding_import_extract') return [{ sourceName: 'profile.json', text: '{}', sourceProvenance: 'stable' }]
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByText('Retrying in background')).toBeInTheDocument()
    expect(screen.queryByText('redacted')).not.toBeInTheDocument()
    expect(screen.getByTestId('onboarding-picker')).toBeEnabled()
    expect(screen.getByTestId('onboarding-confirm')).toBeEnabled()
    await fireEvent.click(screen.getByTestId('onboarding-confirm'))
    expect(screen.getByTestId('onboarding-import-picker')).toBeEnabled()
    expect(screen.getByTestId('onboarding-import-skip')).toBeEnabled()
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    await fireEvent.click(await screen.findByRole('checkbox'))
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    expect(await screen.findByTestId('onboarding-triage-generate')).toBeDisabled()
    expect(screen.getByText('Available when the local AI model is ready.')).toBeInTheDocument()
    expect(invoke.mock.calls.map(([command]) => command)).not.toContain('onboarding_triage')
  })

  it('cleans up active model polling when onboarding is unmounted', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    requiredModelInvoke.mockResolvedValue({ status: { state: 'installing' }, downloadedBytes: 0, totalBytes: 100, folderSetupAvailable: true, aiFeaturesAvailable: false, retryingInBackground: true })
    const view = render(App)
    await screen.findByText('Downloading')
    view.unmount()
    await vi.advanceTimersByTimeAsync(2000)
    expect(requiredModelInvoke).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })

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
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    expect(await screen.findByText('Review an assistant export')).toBeInTheDocument()
    expect(screen.getByText(/Preview happens locally and is read-only/)).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
  })

  it('previews the explicitly selected ZIP and lists its bounded manifest', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
      if (command === 'onboarding_import_preview') return {
        entries: [
          { name: 'conversations/chat.md', kind: 'markdown', byteSize: 128, excerpt: '# Original\nVerbatim text', excerptTruncated: false },
          { name: 'profile.json', kind: 'json', byteSize: 5000, excerpt: '{"name":"A…', excerptTruncated: true },
        ],
        totalByteSize: 5128,
      }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('onboarding_import_preview', expect.anything())
    await fireEvent.click(await screen.findByTestId('onboarding-import-picker'))
    expect(invoke).toHaveBeenCalledWith('onboarding_import_preview', { archivePath: '/Exports/assistant.zip' })
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    const manifest = await screen.findByRole('list', { name: 'Export manifest' })
    expect(within(manifest).getByText('conversations/chat.md')).toBeInTheDocument()
    expect(within(manifest).getByText('markdown · 128 B · complete excerpt')).toBeInTheDocument()
    expect(within(manifest).getAllByText((_, element) => element.tagName === 'PRE' && element.textContent === '# Original\nVerbatim text')).toHaveLength(1)
    expect(within(manifest).getByText('json · 4.9 KB · excerpt truncated')).toBeInTheDocument()
    expect(screen.getByText('2 · 5.0 KB expanded')).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
  })

  it('extracts exactly checked entries and advances only to transient pre-triage', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    const extracted = [{ sourceName: 'profile.json', kind: 'json', text: '{"name":"Alice"}', sourceProvenance: 'assistant-export-zip:v1:stable' }]
    invoke.mockImplementation(async (command) => {
      if (command === 'onboarding_import_preview') return { entries: [
        { name: 'chat.md', kind: 'markdown', byteSize: 10, excerpt: 'chat', excerptTruncated: false },
        { name: 'profile.json', kind: 'json', byteSize: 16, excerpt: '{}', excerptTruncated: false },
      ], totalByteSize: 26 }
      if (command === 'onboarding_import_extract') return extracted
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    const checks = await screen.findAllByRole('checkbox')
    expect(checks).toHaveLength(2)
    expect(checks.every((checkbox) => !checkbox.checked)).toBe(true)
    expect(screen.getByText(/0 of 2 selected/)).toBeInTheDocument()
    expect(screen.getByTestId('onboarding-import-continue')).toBeDisabled()
    await fireEvent.click(checks[1])
    expect(screen.getByText(/1 of 2 selected/)).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    expect(invoke).toHaveBeenCalledWith('onboarding_import_extract', {
      archivePath: '/Exports/assistant.zip', selectedNames: ['profile.json'],
    })
    expect(await screen.findByText('Create your local proposal')).toBeInTheDocument()
    expect(screen.getByText(/Generate a local proposal from 1 approved file/)).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
  })

  it('saves a confirmed local triage report exactly once and completes onboarding', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    const extracted = [{ sourceName: 'profile.json', kind: 'json', text: '{"role":"writer"}', sourceProvenance: 'assistant-export-zip:v1:stable' }]
    let resolveTriage
    const save = deferred()
    const report = { userType: 'Writer', proposedHomeLayout: 'Projects organized by topic.', starterAgents: ['Researcher', 'Editor'] }
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'onboarding_import_preview') return { entries: [{ name: 'profile.json', kind: 'json', byteSize: 17, excerpt: '{}', excerptTruncated: false }], totalByteSize: 17 }
      if (command === 'onboarding_import_extract') return extracted
      if (command === 'onboarding_triage') return new Promise((resolve) => { resolveTriage = resolve })
      if (command === 'home_confirm_import') return save.promise
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    await fireEvent.click(await screen.findByRole('checkbox'))
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    const generate = await screen.findByTestId('onboarding-triage-generate')
    await fireEvent.click(generate)
    await fireEvent.click(generate)
    expect(invoke.mock.calls.filter(([command]) => command === 'onboarding_triage')).toEqual([['onboarding_triage', { entries: extracted }]])
    expect(generate).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent('Generating proposal on this device')
    resolveTriage({ report, usage: null })
    expect(await screen.findByRole('heading', { name: 'User type' })).toBeInTheDocument()
    expect(screen.getByText('Writer')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Proposed Home layout' })).toBeInTheDocument()
    expect(screen.getByText('Projects organized by topic.')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Starter agents' })).toBeInTheDocument()
    expect(screen.getByText('Researcher')).toBeInTheDocument()
    expect(screen.getByText('profile.json')).toBeInTheDocument()
    expect(screen.getByText('assistant-export-zip:v1:stable')).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-triage-confirm'))
    const saveButton = await screen.findByTestId('onboarding-import-save')
    await fireEvent.click(saveButton)
    await fireEvent.click(saveButton)
    expect(saveButton).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent('Saving your Home and approved files')
    expect(invoke.mock.calls.filter(([command]) => command === 'home_confirm_import')).toEqual([[
      'home_confirm_import',
      { homePath: '/Documents/Muniment', triageReport: report, approvedEntries: extracted },
    ]])
    expect(invoke.mock.calls.map(([command]) => command)).not.toContain('home_confirm')
    save.resolve({ configured: true, importedFileCount: 1 })
    await waitFor(() => expect(screen.queryByTestId('onboarding-import-save')).not.toBeInTheDocument())
  })

  it.each([
    ['saveFailed', 'The import could not be saved', true],
    ['invalidInput', 'The confirmed proposal is no longer valid', false],
  ])('shows a redacted, accessible %s recovery path', async (kind, expected, retryable) => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    const extracted = [{ sourceName: 'profile.json', kind: 'json', text: '{}', sourceProvenance: 'stable' }]
    const report = { userType: 'Writer', proposedHomeLayout: 'Projects', starterAgents: ['Researcher'] }
    invoke.mockImplementation(async (command) => {
      if (command === 'onboarding_import_preview') return { entries: [{ name: 'profile.json', kind: 'json', byteSize: 2, excerpt: '{}', excerptTruncated: false }], totalByteSize: 2 }
      if (command === 'onboarding_import_extract') return extracted
      if (command === 'onboarding_triage') return { report }
      if (command === 'home_confirm_import') throw { kind, message: 'sensitive backend detail' }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    await fireEvent.click(await screen.findByRole('checkbox'))
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    await fireEvent.click(await screen.findByTestId('onboarding-triage-generate'))
    await fireEvent.click(await screen.findByTestId('onboarding-triage-confirm'))
    await fireEvent.click(await screen.findByTestId('onboarding-import-save'))
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent(expected)
    expect(alert).not.toHaveTextContent('sensitive backend detail')
    if (retryable) expect(screen.getByTestId('onboarding-import-save')).toBeEnabled()
    else expect(screen.getByTestId('onboarding-import-recover')).toBeEnabled()
  })

  it('identifies a destination conflict and retries retained inputs with a different Home', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    const extracted = [{ sourceName: 'profile.json', kind: 'json', text: '{}', sourceProvenance: 'stable' }]
    const report = { userType: 'Writer', proposedHomeLayout: 'Projects', starterAgents: ['Researcher'] }
    let saveAttempts = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'onboarding_import_preview') return { entries: [{ name: 'profile.json', kind: 'json', byteSize: 2, excerpt: '{}', excerptTruncated: false }], totalByteSize: 2 }
      if (command === 'onboarding_import_extract') return extracted
      if (command === 'onboarding_triage') return { report }
      if (command === 'home_confirm_import' && saveAttempts++ === 0) throw { kind: 'destinationConflict', relativePath: 'memory/profile.md', message: '/private/absolute/path' }
      if (command === 'home_confirm_import') return { configured: true, importedFileCount: 1 }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    await fireEvent.click(await screen.findByRole('checkbox'))
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    await fireEvent.click(await screen.findByTestId('onboarding-triage-generate'))
    await fireEvent.click(await screen.findByTestId('onboarding-triage-confirm'))
    await fireEvent.click(await screen.findByTestId('onboarding-import-save'))
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('memory/profile.md')
    expect(alert).not.toHaveTextContent('/private/absolute/path')
    dialogResult = '/Other/Muniment'
    await fireEvent.click(screen.getByTestId('onboarding-confirmed-picker'))
    expect(await screen.findByTestId('onboarding-home-path')).toHaveTextContent('/Other/Muniment')
    await fireEvent.click(screen.getByTestId('onboarding-import-save'))
    expect(invoke.mock.calls.filter(([command]) => command === 'home_confirm_import')).toEqual([
      ['home_confirm_import', { homePath: '/Documents/Muniment', triageReport: report, approvedEntries: extracted }],
      ['home_confirm_import', { homePath: '/Other/Muniment', triageReport: report, approvedEntries: extracted }],
    ])
    expect(invoke.mock.calls.filter(([command]) => ['onboarding_import_preview', 'onboarding_import_extract', 'onboarding_triage'].includes(command))).toHaveLength(3)
  })

  it('retries a redacted triage failure and suppresses its stale result after returning', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    const extracted = [{ sourceName: 'chat.md', kind: 'markdown', text: 'chat', sourceProvenance: 'stable' }]
    let triageAttempt = 0
    let resolveRetry
    invoke.mockImplementation(async (command) => {
      if (command === 'onboarding_import_preview') return { entries: [{ name: 'chat.md', kind: 'markdown', byteSize: 4, excerpt: 'chat', excerptTruncated: false }], totalByteSize: 4 }
      if (command === 'onboarding_import_extract') return extracted
      if (command === 'onboarding_triage') {
        triageAttempt += 1
        if (triageAttempt === 1) throw { kind: 'invalidModelResponse', message: 'private model output' }
        return new Promise((resolve) => { resolveRetry = resolve })
      }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    await fireEvent.click(await screen.findByRole('checkbox'))
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    await fireEvent.click(await screen.findByTestId('onboarding-triage-generate'))
    expect(await screen.findByRole('alert')).toHaveTextContent('Local AI returned an incomplete proposal. Try generating it again.')
    expect(screen.getByRole('alert')).not.toHaveTextContent('private model output')
    expect(screen.getByText('chat.md')).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-triage-generate'))
    await fireEvent.click(screen.getByTestId('onboarding-triage-back'))
    expect(await screen.findByRole('checkbox')).toBeChecked()
    resolveRetry({ report: { userType: 'Writer', proposedHomeLayout: 'Layout', starterAgents: ['A', 'B'] }, usage: null })
    await Promise.resolve()
    expect(screen.queryByRole('heading', { name: 'User type' })).not.toBeInTheDocument()
    expect(screen.getByRole('checkbox')).toBeChecked()
    expect(invoke.mock.calls.filter(([command]) => command === 'onboarding_triage')).toHaveLength(2)
  })

  it('keeps consent for extraction retry and suppresses stale extraction after skip', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    let extractionAttempt = 0
    let resolveExtraction
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'onboarding_import_preview') return { entries: [
        { name: 'chat.md', kind: 'markdown', byteSize: 4, excerpt: 'chat', excerptTruncated: false },
      ], totalByteSize: 4 }
      if (command === 'onboarding_import_extract') {
        extractionAttempt += 1
        if (extractionAttempt === 1) throw { kind: 'invalidArchive', message: 'detail' }
        return new Promise((resolve) => { resolveExtraction = resolve })
      }
      if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    await fireEvent.click(await screen.findByRole('checkbox'))
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    expect(await screen.findByRole('alert')).toHaveTextContent('approved files could not be read')
    expect(screen.getByRole('checkbox')).toBeChecked()
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    resolveExtraction([{ sourceName: 'chat.md', text: 'chat', sourceProvenance: 'stable' }])
    await Promise.resolve()
    expect(screen.queryByText('Create your local proposal')).not.toBeInTheDocument()
  })

  it('does not preview on picker cancel and can continue without importing', async () => {
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
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(await screen.findByTestId('onboarding-import-picker'))
    expect(invoke).not.toHaveBeenCalledWith('onboarding_import_preview', expect.anything())
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
  })

  it('does not let a pending preview reopen onboarding after skip', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/slow.zip'
    let resolvePreview
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
      if (command === 'onboarding_import_preview') return new Promise((resolve) => { resolvePreview = resolve })
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(await screen.findByTestId('onboarding-import-picker'))
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    resolvePreview({ entries: [], totalByteSize: 0 })
    await Promise.resolve()
    expect(screen.queryByText('Review an assistant export')).not.toBeInTheDocument()
  })

  it('renders a typed rejection and preserves Home while choosing another archive', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/not-an-export.zip'
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
      if (command === 'onboarding_import_preview') throw { kind: 'invalidArchive', message: 'backend detail' }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(await screen.findByTestId('onboarding-import-picker'))
    expect(await screen.findByRole('alert')).toHaveTextContent('That file is not a readable ZIP archive. Choose a different export ZIP.')
    expect(screen.getByText('/Exports/not-an-export.zip')).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
  })

  it('surfaces a scaffold failure and lets the user choose again', async () => {
    homeStatus = { configured: false, homePath: '/read-only/Muniment' }
    dialogResult = '/Documents/Muniment'
    invoke.mockRejectedValue('Muniment Home could not be created.')
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
    await fireEvent.click(await screen.findByTestId('onboarding-import-skip'))
    expect(await screen.findByRole('alert')).toHaveTextContent('Muniment Home could not be created.')
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
    expect(screen.getByText('No ZIP selected')).toBeInTheDocument()
  })

  it('cancels Home settings back to the configured workspace', async () => {
    render(App)
    await fireEvent.click(await screen.findByText('Home settings'))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    expect(screen.queryByText('Local AI')).not.toBeInTheDocument()
    expect(screen.queryByText('Starting setup')).not.toBeInTheDocument()
    expect(requiredModelInvoke).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByTestId('onboarding-picker'))
    await fireEvent.click(screen.getByTestId('onboarding-cancel'))
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke).not.toHaveBeenCalledWith('home_confirm', expect.anything())
  })
})

describe('voice dictation', () => {
  it('routes one global press and matching release through the existing dictation lifecycle', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
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
    await screen.findByPlaceholderText('Ask anything')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByPlaceholderText('Ask anything')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
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
      if (command === 'chat_history') return []
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

  it('preserves the polished draft and ignores global presses while polish is busy', async () => {
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
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Keep this' } })
    globalShortcutHandler({ state: 'Pressed' })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_start'))
    dictationListener({ payload: { type: 'transcript', text: 'captured words' } })
    globalShortcutHandler({ state: 'Released' })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_polish', { transcript: 'captured words' }))
    globalShortcutHandler({ state: 'Pressed' })
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    expect(composer).toHaveValue('Keep this captured words')
    resolvePolish('Polished words')
    await waitFor(() => expect(composer).toHaveValue('Keep this Polished words'))
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
    const profile = await screen.findByRole('button', { name: /Alice/i })
    await fireEvent.click(profile)
    const dialog = screen.getByRole('dialog', { name: 'Your access' })
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

  it('keeps the previous voice binding when replacement registration collides', async () => {
    registerGlobalShortcut.mockImplementation(async (shortcut, handler) => {
      if (shortcut === 'Control+Alt+K') throw new Error('owned by SecretApp.exe')
      globalShortcutHandler = handler
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Your access' })
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Your access' })
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Your access' })
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const capture = within(screen.getByRole('dialog', { name: 'Your access' })).getByRole('button', { name: /Change voice shortcut/ })
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Your access' })
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

  it('offers all transforms after polish and replaces only the captured segment', async () => {
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return 'Polished capture.'
      if (command === 'dictation_transform') return payload.transform === 'key-points' ? '• Captured point' : 'Formal capture.'
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Existing draft' } })
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    expect(screen.queryByRole('button', { name: /key points/ })).not.toBeInTheDocument()
    dictationListener({ payload: { type: 'transcript', text: 'captured words' } })
    await stopClickCapture(voice)

    const chips = await screen.findByLabelText('Voice transforms')
    for (const [label, shortcut] of [['key points', 'Alt+1'], ['formal', 'Alt+2'], ['short', 'Alt+3'], ['long', 'Alt+4']]) {
      expect(within(chips).getByRole('button', { name: new RegExp(label) })).toHaveAttribute('aria-keyshortcuts', shortcut)
    }
    const keyPoints = within(chips).getByRole('button', { name: /key points/ })
    await fireEvent.click(keyPoints)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_transform', { transform: 'key-points', transcript: 'Polished capture.' }))
    await waitFor(() => expect(composer).toHaveValue('Existing draft • Captured point'))
    expect(composer).toHaveFocus()

    await fireEvent.keyDown(composer, { key: '¡', code: 'Digit2', altKey: true })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_transform', { transform: 'formal', transcript: '• Captured point' }))
    await waitFor(() => expect(composer).toHaveValue('Existing draft Formal capture.'))
    expect(composer).toHaveFocus()
  })

  it('expires transform actions after six seconds without letting an old timer hide a newer capture', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    let polishCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return `Polished capture ${++polishCalls}`
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })

    await fireEvent.click(voice)
    dictationListener({ payload: { type: 'transcript', text: 'first' } })
    await stopClickCapture(voice)
    expect(await screen.findByLabelText('Voice transforms')).toBeInTheDocument()

    await vi.advanceTimersByTimeAsync(3000)
    await fireEvent.input(composer, { target: { value: 'Edited between captures' } })
    await fireEvent.click(voice)
    dictationListener({ payload: { type: 'transcript', text: 'second' } })
    await stopClickCapture(voice)
    expect(await screen.findByLabelText('Voice transforms')).toBeInTheDocument()

    await vi.advanceTimersByTimeAsync(3000)
    expect(screen.getByLabelText('Voice transforms')).toBeInTheDocument()
    await vi.advanceTimersByTimeAsync(2500)
    expect(screen.getByLabelText('Voice transforms')).toBeInTheDocument()
    await vi.advanceTimersByTimeAsync(500)
    expect(screen.queryByLabelText('Voice transforms')).not.toBeInTheDocument()
  })

  it('blocks duplicate actions and rejects a transform result after editing', async () => {
    let resolveTransform
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return 'Editable capture'
      if (command === 'dictation_transform') return new Promise((resolve) => { resolveTransform = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    dictationListener({ payload: { type: 'transcript', text: 'capture' } })
    await stopClickCapture(voice)
    const formal = await screen.findByRole('button', { name: /formal/ })
    await fireEvent.click(formal)

    expect(formal).toBeDisabled()
    expect(voice).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
    await fireEvent.click(formal)
    await fireEvent.keyDown(composer, { key: '2', altKey: true })
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_transform')).toHaveLength(1)
    await fireEvent.input(composer, { target: { value: 'My newer edit' } })
    expect(screen.queryByLabelText('Voice transforms')).not.toBeInTheDocument()
    resolveTransform('Late replacement')
    await Promise.resolve()
    await waitFor(() => expect(screen.queryByText('Transforming on this device…')).not.toBeInTheDocument())
    expect(composer).toHaveValue('My newer edit')
    expect(composer).toHaveFocus()
  })

  it('ignores the global shortcut while a transform is pending', async () => {
    let resolveTransform
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return 'Polished capture'
      if (command === 'dictation_transform') return new Promise((resolve) => { resolveTransform = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Existing draft' } })
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    await waitFor(() => expect(voice).toHaveAttribute('aria-pressed', 'true'))
    dictationListener({ payload: { type: 'transcript', text: 'captured words' } })
    await stopClickCapture(voice)
    await fireEvent.click(await screen.findByRole('button', { name: /formal/ }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_transform', { transform: 'formal', transcript: 'Polished capture' }))

    invoke.mockClear()
    globalShortcutHandler({ state: 'Pressed' })
    globalShortcutHandler({ state: 'Released' })
    expect(invoke).not.toHaveBeenCalledWith('dictation_start')
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(0)
    expect(composer).toHaveValue('Existing draft Polished capture')

    resolveTransform('Formal capture')
    await waitFor(() => expect(composer).toHaveValue('Existing draft Formal capture'))
  })

  it('keeps text and reports a redacted transform failure, then cancels late work with Escape', async () => {
    let transformCalls = 0
    let resolveLate
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return 'Keep polished text'
      if (command === 'dictation_transform') {
        transformCalls += 1
        if (transformCalls === 1) throw { category: 'requestFailed', message: 'sensitive backend detail' }
        return new Promise((resolve) => { resolveLate = resolve })
      }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    dictationListener({ payload: { type: 'transcript', text: 'keep words' } })
    await stopClickCapture(voice)
    await fireEvent.click(await screen.findByRole('button', { name: /short/ }))
    expect(await screen.findByRole('alert')).toHaveTextContent('That voice transform is unavailable. Your text is unchanged; try again.')
    expect(screen.getByRole('alert')).not.toHaveTextContent('sensitive backend detail')
    expect(composer).toHaveValue('Keep polished text')

    await fireEvent.click(screen.getByRole('button', { name: /long/ }))
    await fireEvent.keyDown(document, { key: 'Escape' })
    expect(composer).toHaveValue('Keep polished text')
    expect(screen.queryByLabelText('Voice transforms')).not.toBeInTheDocument()
    expect(composer).toHaveFocus()
    resolveLate('Late after Escape')
    await Promise.resolve()
    expect(composer).toHaveValue('Keep polished text')
  })

  it('starts on primary pointer down and stops on release while preserving transcript', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    let resolveStop
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return new Promise((resolve) => { resolveStop = resolve })
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
    await vi.advanceTimersByTimeAsync(300)

    resolveStop({ state: 'stopped' })
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(invoke).not.toHaveBeenCalledWith('dictation_polish', expect.anything())
    expect(voice).toBeDisabled()
    await fireEvent.click(voice)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    dictationListener({ payload: { type: 'transcript', text: 'at the boundary' } })

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('dictation_polish', { transcript: 'after at the boundary' }))
    await waitFor(() => expect(composer).toHaveValue('Before Polished after.'))
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_start')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_stop')).toHaveLength(1)
    expect(invoke.mock.calls.filter(([command]) => command === 'dictation_polish')).toHaveLength(1)
  })

  it('promotes a rapid button double activation to hands-free and Escape restores the draft during a late start', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    let resolveStart
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return new Promise((resolve) => { resolveStart = resolve })
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
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
    await stopClickCapture(voice)

    expect(await screen.findByRole('alert')).toHaveTextContent('Polishing is unavailable. You can edit or send the captured text.')
    expect(composer).toHaveValue('Keep verbatim words')
    expect(composer).not.toHaveAttribute('readonly')
    expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled()
  })

  it('styles only the captured segment and rejects stale capture completions while mounted', async () => {
    let resolvePolish
    let starts = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_history') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') {
        starts += 1
        return { state: 'running' }
      }
      if (command === 'dictation_stop') return { state: 'stopped' }
      if (command === 'dictation_polish') return new Promise((resolve) => { resolvePolish = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.input(composer, { target: { value: 'Existing draft' } })
    await fireEvent.click(voice)
    const oldCaptureListener = dictationListener
    dictationListener({ payload: { type: 'transcript', text: 'old capture' } })
    await stopClickCapture(voice)

    expect(await screen.findByRole('status')).toHaveTextContent('Polishing on this device…')
    expect(screen.getByTestId('polish-draft').textContent).toBe('Existing draft ')
    expect(screen.getByTestId('polish-draft')).not.toHaveClass('polish-transcript')
    expect(screen.getByTestId('polish-transcript')).toHaveTextContent('old capture')
    expect(screen.getByTestId('polish-transcript')).toHaveClass('polish-transcript')
    expect(voice).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
    expect(composer).toHaveAttribute('readonly')

    await fireEvent.keyDown(document, { key: 'Escape' })
    await fireEvent.input(composer, { target: { value: 'Newer draft exactly' } })
    await fireEvent.click(voice)
    await waitFor(() => expect(starts).toBe(2))
    oldCaptureListener({ payload: { type: 'transcript', text: 'late old transcript' } })
    resolvePolish('Stale replacement')
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(composer).toHaveValue('Newer draft exactly')
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
    await stopClickCapture(voice)

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

    await stopClickCapture(voice)
    expect(invoke).toHaveBeenCalledWith('dictation_stop')
    expect(voice).toHaveAttribute('aria-pressed', 'false')
    await waitFor(() => expect(send).toBeEnabled())
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

    await stopClickCapture(voice)
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
    expect(saved).toHaveTextContent('lease.pdf1.0 KBSaved locally · supported images sent with first prompt')
    expect(saved).not.toHaveTextContent(/not sent to (?:the )?model/i)
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
  expect(saved).toHaveTextContent('contract.pdf214 KBSaved locally · supported images sent with first prompt')
  expect(saved).not.toHaveTextContent(/not sent to (?:the )?model/i)
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
  expect(saved).toHaveTextContent('evidence.txt1.5 KBSaved locally · supported images sent with first prompt')
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

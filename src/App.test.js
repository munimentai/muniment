// @vitest-environment jsdom

import fs from 'node:fs'
import path from 'node:path'

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
const accessPanelSource = fs.readFileSync(path.join(process.cwd(), 'src/lib/AccessPanel.svelte'), 'utf8')

let App
let invoke
let chatListener
let dictationListener
let entitlementListener
let registrationRetryListener
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
let globalShortcutHandler
let registerGlobalShortcut
let unregisterGlobalShortcut
let registeredShortcuts
let threadSummaryResult
let olderThreadSummaryResult

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
      return command === 'home_status'
        ? Promise.resolve(homeStatus)
        : invoke(command, ...args)
    } },
    event: { listen: vi.fn((event, listener) => {
      if (event === 'chat-event') chatListener = listener
      if (event === 'dictation-event') dictationListener = listener
      if (event === 'entitlement-changed') entitlementListener = listener
      if (event === 'auth-registration-retry') registrationRetryListener = listener
      if (event === 'attach-pairing-requested') pairingListener = listener
      if (event === 'attach-pairing-requested' && pairingRegistrationError) {
        return Promise.reject(pairingRegistrationError)
      }
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
  threadSummaryResult = [{ threadId: 'thread-1', title: '', updatedAt: '' }]
  olderThreadSummaryResult = null
  homeStatus = { configured: true, homePath: '/Documents/Muniment' }
  chatListener = undefined
  dictationListener = undefined
  entitlementListener = undefined
  registrationRetryListener = undefined
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
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_thread_open') return []
    if (command === 'chat_file_metadata') return { displayName: payload.path.split(/[\\/]/).pop(), byteLength: 1536 }
    if (command === 'auth_entitlement_snapshot') return snapshot()
    if (command === 'auth_devices') return []
    if (command === 'attach_companions') return []
    if (command === 'attach_listener_status') return { started: true, failure: null }
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'auth_sign_out') return { signed_in: false, subject: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const profile = await screen.findByRole('button', { name: /Alice/i })
    entitlementListener({ payload: { snapshot_version: 3 } })
    expect(await screen.findByText(copy)).toBeInTheDocument()

    await fireEvent.click(profile)
    await fireEvent.click(screen.getByRole('button', { name: 'Sign out' }))

    expect(await screen.findByRole('button', { name: 'Sign in' })).toBeInTheDocument()
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
    const composer = await screen.findByRole('textbox', { name: 'Message' })
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
  it('names and describes the composer in its default state', async () => {
    render(App)

    const composer = await screen.findByRole('textbox', { name: 'Message' })
    expect(composer).toHaveAccessibleDescription('Routing is automatic. Every reply carries its receipt.')
    expect(composer).toHaveAttribute('placeholder', 'Ask anything')
  })

  it('renders only the sidebar brand in the signed-in workspace', async () => {
    const { container } = render(App)

    await screen.findByPlaceholderText('Ask anything')

    expect(container.querySelector('.lockup')).not.toBeInTheDocument()
    expect(screen.queryByText(/shell v/)).not.toBeInTheDocument()
    expect(container.querySelectorAll('.side-brand')).toHaveLength(1)
    expect(screen.getAllByText('muniment')).toHaveLength(1)
  })

  it('focuses the primary composer action once when the workspace appears', async () => {
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    const send = screen.getByRole('button', { name: 'Send' })

    expect(composer).toHaveFocus()
    expect(send).toHaveClass('primary')
    expect(send).toHaveAttribute('aria-disabled', 'true')
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

  it('does not focus a composer outside the signed-in workspace', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    const signIn = await screen.findByRole('button', { name: 'Sign in' })
    expect(signIn).not.toHaveFocus()
    expect(signIn).toHaveClass('primary')
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1)
    expect(screen.getByRole('heading', { level: 1, name: 'muniment' })).toBeInTheDocument()
    expect(document.querySelector('.lockup svg')).toHaveAttribute('aria-hidden', 'true')
    expect(document.querySelector('.lockup svg')).not.toHaveAttribute('aria-label')
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
  })

  it('keeps one focused sign-in button while browser sign-in is pending', async () => {
    const signInRequest = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'auth_sign_in') return signInRequest.promise
      throw new Error(`unexpected command: ${command}`)
    })
    const { container } = render(App)
    const signIn = await screen.findByRole('button', { name: 'Sign in' })
    expect(screen.getByText('Sign in to continue to your workspace.')).toHaveAttribute('aria-live', 'polite')
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
    expect(container.querySelectorAll('button')).toHaveLength(1)
    expect(appRules.get('button[aria-disabled="true"].inactive')).toMatch(/color:\s*var\(--muted\)/)
    expect(appRules.get('button[aria-disabled="true"].inactive')).toMatch(/cursor:\s*default/)
    expect(appRules.has('button:hover:not(:disabled):not([aria-disabled="true"])')).toBe(true)

    await fireEvent.click(signIn)
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_sign_in')).toHaveLength(1)
  })

  it('shows the registration wait and completes without another user action', async () => {
    const signInRequest = deferred()
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'auth_sign_in') return signInRequest.promise
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }))
    await waitFor(() => expect(registrationRetryListener).toBeDefined())

    registrationRetryListener({ payload: { delay_seconds: 30 } })

    expect(await screen.findByText('Server busy. Retrying in 30 s')).toBeInTheDocument()
    expect(screen.queryByText(/Sign-in not completed/)).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument()

    signInRequest.resolve({ signed_in: true, subject: 'user-a' })
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke.mock.calls.filter(([command]) => command === 'auth_sign_in')).toHaveLength(1)
  })

  it('shows the terminal screen for a non-retryable registration error', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: false, subject: null }
      if (command === 'auth_sign_in') throw new Error('native installation registration failed')
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)

    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }))

    expect(await screen.findByText(/Sign-in not completed: Error: native installation registration failed/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument()
  })

  it('does not focus a composer in the auth-error state', async () => {
    invoke.mockRejectedValue('Authentication is unavailable.')
    render(App)

    expect(await screen.findByRole('button', { name: 'Try again' })).not.toHaveFocus()
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
  })

  it('does not focus a composer during onboarding', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    render(App)

    expect(await screen.findByRole('heading', { name: 'Choose your Muniment Home' })).toBeInTheDocument()
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1)
    expect(document.querySelector('.lockup .name')?.tagName).toBe('SPAN')
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
  })

  it('releases composer focus in Home settings and restores it on workspace re-entry', async () => {
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    expect(composer).toHaveFocus()

    await fireEvent.click(screen.getByText('Home settings'))
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-cancel'))

    expect(await screen.findByPlaceholderText('Ask anything')).toHaveFocus()
  })

  it('waits to focus on workspace re-entry until a resuming composer becomes enabled', async () => {
    let resolveResume
    invoke.mockImplementation(async (command) => {
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
    await fireEvent.click(screen.getByText('Home settings'))
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
    expect(within(toggle).getByText(navigator.platform.startsWith('Mac') ? '⌘J' : 'Ctrl J').tagName).toBe('KBD')
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

  it('exposes an operable window splitter and resets its width after closing', async () => {
    render(App)
    const toggle = await screen.findByRole('button', { name: 'Open artifact rail' })
    await fireEvent.click(toggle)
    const separator = screen.getByRole('separator', { name: 'Artifacts' })
    expect(separator).toHaveAttribute('tabindex', '0')
    expect(separator).toHaveAttribute('aria-orientation', 'vertical')
    expect(separator).toHaveAttribute('aria-valuemin', '380')
    expect(separator).toHaveAttribute('aria-valuemax', '444')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
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
    await fireEvent.click(screen.getByRole('button', { name: 'Sign out' }))
    await screen.findByRole('button', { name: 'Sign in' })
    await fireEvent.keyDown(document, shortcut)
    await fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(await screen.findByRole('button', { name: 'Open artifact rail' })).toHaveAttribute('aria-expanded', 'false')
  })

  it('ignores the rail shortcut during signed-in onboarding', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    render(App)
    await screen.findByRole('heading', { name: 'Choose your Muniment Home' })
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

describe('thread name', () => {
  it('exposes the workspace heading, thread list, and named transcript region', async () => {
    threadSummaryResult = [
      { threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' },
      { threadId: 'thread-2', title: 'Archive review', updatedAt: '' },
    ]
    render(App)

    const heading = await screen.findByRole('heading', { level: 1, name: 'Lease renewal' })
    const listHeading = screen.getByRole('heading', { level: 2, name: 'Threads' })
    const list = screen.getByRole('list', { name: 'Threads' })

    expect(heading).toContainElement(screen.getByRole('button', { name: 'Rename thread' }))
    expect(listHeading).toBeInTheDocument()
    expect(within(list).getAllByRole('listitem')).toHaveLength(2)
    expect(screen.getByRole('region', { name: 'Transcript: Lease renewal' })).toBeInTheDocument()
  })

  it('appends older threads, moves focus, and removes the control on the last page', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    olderThreadSummaryResult = [{ threadId: 'thread-2', title: 'Older review', updatedAt: '' }]
    render(App)

    const control = await screen.findByRole('button', { name: 'Older threads' })
    const list = screen.getByRole('list', { name: 'Threads' })
    const homeSettings = screen.getByRole('button', { name: 'Home settings' })

    expect(within(list).getAllByRole('listitem')).toHaveLength(1)
    expect(within(list).queryByRole('button', { name: 'Older threads' })).not.toBeInTheDocument()
    expect(list.compareDocumentPosition(control)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
    expect(control.compareDocumentPosition(homeSettings)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
    await fireEvent.click(control)

    const olderThread = await screen.findByRole('button', { name: 'Older review' })
    expect(olderThread).toHaveFocus()
    expect(screen.queryByRole('button', { name: 'Older threads' })).not.toBeInTheDocument()
  })

  it('reveals a quiet delete action and cancels its inline confirmation', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    render(App)
    const remove = await screen.findByRole('button', { name: 'Delete Lease renewal' })

    expect(remove.tabIndex).toBe(0)
    expect(appRules.get('.thread-delete')).toMatch(/opacity:\s*0/)
    expect(appRules.get('.thread-record:hover .thread-delete, .thread-record:focus-within .thread-delete')).toMatch(/opacity:\s*1/)
    await fireEvent.click(remove)
    expect(screen.getByLabelText('Delete Lease renewal?')).toHaveTextContent('Delete “Lease renewal”?')

    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByLabelText('Delete Lease renewal?')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Delete Lease renewal' })).toHaveFocus()

    await fireEvent.click(screen.getByRole('button', { name: 'Delete Lease renewal' }))
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Delete' }), { key: 'Escape' })
    expect(screen.queryByLabelText('Delete Lease renewal?')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Delete Lease renewal' })).toHaveFocus()
  })

  it('deletes the open thread and focuses the fresh composer', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{ runId: 'run-1', phase: 'complete', prompt: 'Question', text: 'Answer', toolActivity: [] }]
      if (command === 'chat_delete_thread') return undefined
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await screen.findByText('Answer')

    await fireEvent.click(screen.getByRole('button', { name: 'Delete Lease renewal' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }))

    await waitFor(() => expect(screen.queryByText('Answer')).not.toBeInTheDocument())
    expect(invoke).toHaveBeenCalledWith('chat_delete_thread', { threadId: 'thread-1' })
    expect(document.querySelector('[data-fresh-thread]')).toBeInTheDocument()
    expect(within(screen.getByRole('list', { name: 'Threads' })).getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getByPlaceholderText('Ask anything')).toHaveFocus()
  })

  it('keeps the row and reports a failed delete', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Lease renewal', updatedAt: '' }]
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'chat_delete_thread') throw new Error('offline')
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: 'Delete Lease renewal' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('The thread could not be deleted.')
    expect(screen.getByRole('button', { name: 'Delete Lease renewal' })).toBeInTheDocument()
    expect(screen.queryByLabelText('Delete Lease renewal?')).not.toBeInTheDocument()
  })

  it('uses the stored title and renames it from the keyboard', async () => {
    threadSummaryResult = [{ threadId: 'thread-1', title: 'Stored name', updatedAt: '' }]
    invoke.mockImplementation(async (command) => {
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
      expect(document.querySelector(`time[datetime="${summary.updatedAt}"]`)).toHaveAttribute('title', new Date(summary.updatedAt).toLocaleString())
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
    await screen.findByPlaceholderText('Ask anything')

    const titlebarName = document.querySelector('.thread-title')
    const sidebarName = document.querySelector('.thread-row')
    expect(titlebarName).toHaveTextContent('New thread')
    expect(titlebarName).toHaveAttribute('title', 'New thread')
    expect(sidebarName).toHaveTextContent('New thread')
    expect(sidebarName).toHaveAttribute('title', 'New thread')
    expect(sidebarName.querySelector('time')).toHaveAttribute('title', '')
    expect(screen.queryByText('local · durable')).not.toBeInTheDocument()
  })

  it('shows the first restored prompt in the titlebar and current thread record', async () => {
    invoke.mockImplementation(async (command) => {
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
    expect(titlebarName).toHaveAttribute('title', 'Review the lease renewal')
    expect(sidebarName).toHaveTextContent('Review the lease renewal')
    expect(sidebarName).toHaveAttribute('title', 'Review the lease renewal')
    expect(screen.queryByText('local · durable')).not.toBeInTheDocument()
  })

  it('uses one-line ellipsis styles for both thread names', () => {
    expect(appRules.get('.thread-title')).toMatch(/overflow:\s*hidden/)
    expect(appRules.get('.thread-title')).toMatch(/text-overflow:\s*ellipsis/)
    expect(appRules.get('.thread-title')).toMatch(/white-space:\s*nowrap/)
    expect(appRules.get('.thread-row-title')).toMatch(/overflow:\s*hidden/)
    expect(appRules.get('.thread-row-title')).toMatch(/text-overflow:\s*ellipsis/)
    expect(appRules.get('.thread-row-title')).toMatch(/white-space:\s*nowrap/)
  })

  it('reserves the current-thread dot width in every row', () => {
    expect(appRules.get('.thread-row > span')).toMatch(/flex:\s*0 0 5px/)
  })
})

describe('sidebar collapse', () => {
  const sidebarShortcut = () => navigator.platform.startsWith('Mac')
    ? { key: '\\', metaKey: true }
    : { key: '\\', ctrlKey: true }

  it('collapses to an icon rail from the in-sidebar control and expands again', async () => {
    render(App)
    const collapse = await screen.findByRole('button', { name: 'Collapse sidebar' })
    expect(collapse).toHaveAttribute('aria-expanded', 'true')
    expect(collapse).toHaveAttribute('aria-controls', 'sidebar')
    expect(collapse).toHaveAttribute('aria-keyshortcuts', navigator.platform.startsWith('Mac') ? 'Meta+\\' : 'Control+\\')
    expect(screen.getByText('Threads')).toBeInTheDocument()
    const newThread = document.querySelector('.new-thread')
    expect(newThread).toHaveAttribute('aria-keyshortcuts', navigator.platform.startsWith('Mac') ? 'Meta+N' : 'Control+N')
    expect(newThread.querySelector('kbd')).toHaveTextContent(navigator.platform.startsWith('Mac') ? '⌘N' : 'Ctrl N')
    const currentThread = document.querySelector('.thread-row')
    expect(currentThread).toHaveTextContent('New thread')
    expect(currentThread).toHaveAttribute('aria-current', 'true')
    expect(currentThread).not.toHaveAttribute('tabindex')
    expect(currentThread.tabIndex).toBe(-1)
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument()
    expect(document.querySelectorAll('.side-action')).toHaveLength(2)
    expect(screen.getByRole('button', { name: 'Home settings' })).toHaveTextContent('Home settings')
    expect(document.querySelectorAll('#sidebar kbd')).toHaveLength(1)

    collapse.focus()
    await fireEvent.click(collapse)

    const expand = screen.getByRole('button', { name: 'Expand sidebar' })
    // The toggle is one persistent element, so keyboard focus survives the toggle.
    expect(expand).toBe(collapse)
    expect(document.activeElement).toBe(expand)
    expect(expand).toHaveAttribute('aria-expanded', 'false')
    expect(expand).toHaveAttribute('title', expect.stringContaining('Expand sidebar'))
    expect(screen.getByRole('button', { name: 'New thread' })).toHaveAttribute('title', 'New thread')
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Home settings' })).toHaveAttribute('title', 'Home settings')
    expect(screen.queryByText('Threads')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Alice/i })).not.toBeInTheDocument()

    await fireEvent.click(expand)

    expect(screen.getByRole('button', { name: 'Collapse sidebar' })).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('Threads')).toBeInTheDocument()
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
    await screen.findByRole('heading', { name: 'Choose your Muniment Home' })
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
    expect(screen.getByRole('separator', { name: 'Artifacts' })).toHaveAttribute('aria-valuemax', '444')

    await fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }))
    const separator = screen.getByRole('separator', { name: 'Artifacts' })
    expect(separator).toHaveAttribute('aria-valuemax', '560')

    await fireEvent.keyDown(separator, { key: 'End' })
    expect(separator).toHaveAttribute('aria-valuenow', '560')
    await fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }))
    expect(screen.getByRole('separator', { name: 'Artifacts' })).toHaveAttribute('aria-valuenow', '444')
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
    const composer = await screen.findByPlaceholderText('Ask anything')
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
    await screen.findByPlaceholderText('Ask anything')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'chat_new_thread') return null
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await waitFor(() => expect(document.querySelector('.thread-row[aria-current="true"]')).toBeInTheDocument())
    composer.focus()

    await fireEvent.keyDown(composer, newThreadShortcut())

    await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === 'chat_new_thread')).toHaveLength(1))
    expect(document.activeElement).toBe(composer)
  })

  it('keeps the transcript and current row when the command fails', async () => {
    invoke.mockImplementation(async (command) => {
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
    const composer = await screen.findByPlaceholderText('Ask anything')
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
  it('picks a Home after onboarding fails to load', async () => {
    homeStatus = Promise.reject('The saved Home could not be read.')
    dialogResult = '/Other/Muniment'
    render(App)

    expect(await screen.findByRole('alert')).toHaveTextContent('The saved Home could not be read.')
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-picker'))

    expect(await screen.findByTestId('onboarding-home-path')).toHaveTextContent('/Other/Muniment')
    expect(screen.getByTestId('onboarding-confirm')).toBeEnabled()
    expect(screen.queryByText('The saved Home could not be read.')).not.toBeInTheDocument()
  })

  it('blocks the shell and confirms the displayed Documents default', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    expect(screen.queryByText('Sign in')).not.toBeInTheDocument()
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-confirm'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
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
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    expect(invoke).not.toHaveBeenCalledWith('onboarding_import_preview', expect.anything())
    await fireEvent.click(await screen.findByTestId('onboarding-import-picker'))
    expect(invoke).toHaveBeenCalledWith('onboarding_import_preview', { archivePath: '/Exports/assistant.zip' })
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    const manifest = await screen.findByRole('list', { name: 'Export manifest' })
    expect(within(manifest).getByText('conversations/chat.md')).toBeInTheDocument()
    expect(within(manifest).getByText('markdown · 128 B · complete excerpt')).toBeInTheDocument()
    expect(within(manifest).getAllByText((_, element) => element.tagName === 'PRE' && element.textContent === '# Original\nVerbatim text')).toHaveLength(1)
    expect(within(manifest).getByText('json · 4.9 KB · excerpt truncated')).toBeInTheDocument()
    expect(screen.getByText('2 · 5.0 KB expanded')).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
  })

  it('extracts exactly checked entries and saves the approved originals', async () => {
    homeStatus = { configured: false, homePath: '/Documents/Muniment' }
    dialogResult = '/Exports/assistant.zip'
    const extracted = [{ sourceName: 'profile.json', kind: 'json', text: '{"name":"Alice"}', sourceProvenance: 'assistant-export-zip:v1:stable' }]
    const importSave = deferred()
    invoke.mockImplementation(async (command, payload) => {
      if (command === 'home_confirm') return { configured: true, homePath: '/Documents/Muniment' }
      if (command === 'onboarding_import_preview') return { entries: [
        { name: 'chat.md', kind: 'markdown', byteSize: 10, excerpt: 'chat', excerptTruncated: false },
        { name: 'profile.json', kind: 'json', byteSize: 16, excerpt: '{}', excerptTruncated: false },
      ], totalByteSize: 26 }
      if (command === 'onboarding_import_extract') return extracted
      if (command === 'home_confirm_import') return importSave.promise
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByText('Home location')).toBeInTheDocument()
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
    expect(await screen.findByText('Save approved files')).toBeInTheDocument()
    expect(screen.getByText('Home location')).toBeInTheDocument()
    expect(screen.getByText('1 approved file is ready to save as verbatim originals.')).toBeInTheDocument()
    expect(screen.getByText('profile.json')).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-import-recover'))
    expect(await screen.findByRole('list', { name: 'Export manifest' })).toBeInTheDocument()
    expect(screen.getAllByRole('checkbox')[1]).toBeChecked()
    await fireEvent.click(screen.getByTestId('onboarding-import-continue'))
    expect(await screen.findByText('Save approved files')).toBeInTheDocument()
    await fireEvent.click(screen.getByTestId('onboarding-import-save'))
    expect(screen.getByTestId('onboarding-import-recover')).toBeDisabled()
    expect(screen.getByTestId('onboarding-import-save')).toBeDisabled()
    expect(invoke).toHaveBeenCalledWith('home_confirm_import', {
      homePath: '/Documents/Muniment',
      approvedEntries: extracted,
    })
    importSave.resolve({ configured: true, importedFileCount: 1 })
    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
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
      if (command === 'chat_thread_open') return []
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
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(await screen.findByTestId('onboarding-import-picker'))
    expect(invoke).not.toHaveBeenCalledWith('onboarding_import_preview', expect.anything())
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
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
      if (command === 'chat_thread_open') return []
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
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    await fireEvent.click(await screen.findByTestId('onboarding-import-picker'))
    expect(await screen.findByRole('alert')).toHaveTextContent('That file is not a readable ZIP archive. Choose a different export ZIP.')
    expect(screen.getByText('/Exports/not-an-export.zip')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    await fireEvent.click(screen.getByTestId('onboarding-import-picker'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
    await fireEvent.click(screen.getByTestId('onboarding-import-skip'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/Documents/Muniment' })
  })

  it('surfaces a scaffold failure and lets the user choose again', async () => {
    homeStatus = { configured: false, homePath: '/read-only/Muniment' }
    dialogResult = '/Documents/Muniment'
    invoke.mockRejectedValue('Muniment Home could not be created.')
    render(App)
    await fireEvent.click(await screen.findByTestId('onboarding-confirm'))
    expect(invoke).toHaveBeenCalledWith('home_confirm', { homePath: '/read-only/Muniment' })
    expect(await screen.findByRole('alert')).toHaveTextContent('Muniment Home could not be created.')
    expect(screen.queryByPlaceholderText('Ask anything')).not.toBeInTheDocument()
    expect(screen.getByTestId('onboarding-picker')).toBeEnabled()
  })

  it('cancels Home settings back to the configured workspace', async () => {
    render(App)
    await fireEvent.click(await screen.findByText('Home settings'))
    expect(screen.getByTestId('onboarding-home-path')).toHaveTextContent('/Documents/Muniment')
    expect(screen.queryByText('Local AI')).not.toBeInTheDocument()
    expect(screen.queryByText('Starting setup')).not.toBeInTheDocument()
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
      if (command === 'chat_thread_open') return []
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
      if (command === 'chat_thread_open') return []
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
    const profile = await screen.findByRole('button', { name: /Alice/i })
    await fireEvent.click(profile)
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
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
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
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
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
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
    const capture = within(screen.getByRole('dialog', { name: 'Profile' })).getByRole('button', { name: /Change voice shortcut/ })
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
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return new Promise((resolve) => { resolveStop = resolve })
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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



  it('cancels a primary pointer capture, restores the snapshot, and focuses the composer', async () => {
    let stopping = false
    invoke.mockImplementation(async (command) => {
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
      if (command === 'chat_thread_open') return []
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
    await waitFor(() => expect(within(card).getByRole('status')).toHaveTextContent('Installed. Press Voice again to dictate.'))
    expect(invoke).toHaveBeenCalledWith('parakeet_install_start')
    expect(invoke.mock.calls.filter(([command]) => command === 'parakeet_install_status')).toHaveLength(3)
    expect(within(card).queryByRole('button', { name: 'Install' })).not.toBeInTheDocument()
  })

  it.each([
    ['cancelled', 'Cancelled.'],
    ['failed', 'Failed.'],
  ])('states a %s install and returns the Install control', async (installState, words) => {
    invoke.mockImplementation(async (command) => {
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
    const composer = await screen.findByPlaceholderText('Ask anything')
    const voice = screen.getByRole('button', { name: 'Voice' })
    await fireEvent.click(voice)
    view.unmount()
    expect(eventUnlisten).toHaveBeenCalledTimes(4)
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
    expect(screen.queryByRole('button', { name: 'Queue follow-up' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Send' })).toHaveAttribute('aria-disabled', 'true')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [existingRun]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return new Promise((resolve) => { resolveSubmit = resolve })
      throw new Error(`unexpected command: ${command}`)
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
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

  it('replaces only the matching pending run after a failed submission', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [existingRun]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') throw 'The submission was rejected.'
      throw new Error(`unexpected command: ${command}`)
    })
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'New question' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('The submission was rejected.')
    // Scoped to the transcript: the run announcement carries the same sentence.
    const thread = within(document.querySelector('.thread'))
    expect(thread.getByText('Reply failed.')).toBeInTheDocument()
    expect(thread.getByText('Existing answer')).toBeInTheDocument()
    expectNoProxyEqualityWarning(warn)
  })
})

describe('permission gates', () => {
  function restoreGate(gate, phase = 'pending-permission', answer = vi.fn().mockResolvedValue(undefined)) {
    invoke.mockImplementation(async (command, payload) => {
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
    const hint = screen.getByText(navigator.platform.startsWith('Mac') ? '⌘⏎ submits' : 'Ctrl ⏎ submits')
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

  function signedIn(history, submit) {
    invoke.mockImplementation(async (command) => {
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
  ])('renders the active-line rule only in the %s phase', async (phase, ruleCount) => {
    const { container } = signedIn([{
      runId: `run-${phase}`,
      phase,
      text: 'A response long enough to represent prose.',
      prompt: 'A question',
      receipt: phase === 'complete' ? {} : null,
      toolActivity: [],
      resumable: false,
    }])

    if (phase === 'thinking') await screen.findByLabelText('Thinking')
    else await screen.findByText('A response long enough to represent prose.')
    expect(container.querySelectorAll('.streaming-rule')).toHaveLength(ruleCount)
  })

  it('swaps streaming source text for terminal Markdown', async () => {
    const { container } = signedIn([{
      runId: 'run-markdown', phase: 'streaming', text: '## Draft',
      prompt: 'A question', receipt: null, toolActivity: [], resumable: false,
    }])

    const streaming = await screen.findByText('## Draft')
    expect(streaming).toHaveProperty('tagName', 'P')
    expect(streaming).toHaveClass('response-prose', 'streaming')
    expect(container.querySelector('.caret')).toBeInTheDocument()
    expect(container.querySelector('.streaming-rule')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Draft' })).not.toBeInTheDocument()

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

  it('keeps a paused reply as plain text', async () => {
    const { container } = signedIn([{
      runId: 'run-paused', phase: 'pending-permission', text: '## Not terminal',
      prompt: 'A question', receipt: null, toolActivity: [], resumable: false,
      pendingPermission: null,
    }])

    const reply = await screen.findByText('## Not terminal')
    expect(reply).toHaveProperty('tagName', 'P')
    expect(reply).toHaveClass('response-prose')
    expect(container.querySelector('.assistant-markdown')).not.toBeInTheDocument()
  })

  it('keeps the thinking chip mounted through its eased handoff to streaming', async () => {
    signedIn([{
      runId: 'run-thinking', phase: 'thinking', text: '', prompt: 'A question',
      receipt: null, toolActivity: [], resumable: false,
    }])
    const chip = await screen.findByText('Routing')
    const mark = screen.getByLabelText('Thinking').querySelector('path')
    expect(mark).toHaveAttribute('fill-rule', 'evenodd')
    expect(mark).not.toHaveAttribute('stroke')

    chatListener({ payload: {
      runId: 'run-thinking', phase: 'streaming', text: 'First token',
      receipt: null, toolActivity: [], pendingPermission: null,
    } })

    expect(chip).toBeInTheDocument()
    expect(await screen.findByText('First token')).toBeInTheDocument()
    await waitFor(() => expect(chip).not.toBeInTheDocument(), { timeout: 500 })
  })

  it('removes the thinking chip without motion when reduced motion is preferred', async () => {
    const originalMatchMedia = window.matchMedia
    window.matchMedia = vi.fn(() => ({ matches: true }))
    try {
      signedIn([{
        runId: 'run-reduced', phase: 'thinking', text: '', prompt: 'A question',
        receipt: null, toolActivity: [], resumable: false,
      }])
      const chip = await screen.findByText('Routing')

      chatListener({ payload: {
        runId: 'run-reduced', phase: 'streaming', text: 'First token',
        receipt: null, toolActivity: [], pendingPermission: null,
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
    await fireEvent.click(screen.getByRole('button', { name: 'Sign out' }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }))

    expect(await screen.findByText('Restored answer')).toBeInTheDocument()
    expect(screen.getByTestId('run-announcement').textContent).toBe('')
  })

  it('announces the completion of a reply that was restored mid-stream', async () => {
    signedIn([{ runId: 'run-live', phase: 'streaming', text: 'Half an', prompt: 'A question', receipt: null, toolActivity: [] }])
    expect(await screen.findByText('Half an')).toBeInTheDocument()
    const region = screen.getByTestId('run-announcement')
    expect(region.textContent).toBe('')
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))
    const drain = watch(region)

    chatListener({ payload: { runId: 'run-live', phase: 'complete', text: 'Half an answer', receipt: {}, toolActivity: [] } })

    await waitFor(() => expect(region).toHaveTextContent('Reply complete. Half an answer'))
    expect(drain()).toHaveLength(1)
  })

  it('stays silent when a stray event lands on an already settled restored run', async () => {
    signedIn(restored)
    expect(await screen.findByText('Restored answer')).toBeInTheDocument()
    const region = screen.getByTestId('run-announcement')
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))
    const drain = watch(region)

    chatListener({ payload: { runId: 'run-old', phase: 'complete', text: 'Restored answer', receipt: {}, toolActivity: [{ effectId: 'tool-1', displayName: 'Read file', status: 'completed' }] } })

    expect(await screen.findByRole('listitem', { name: 'Read file completed' })).toBeInTheDocument()
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

describe('tool activity cards', () => {
  const statuses = [
    ['running', 'running'],
    ['completed', 'completed'],
    ['failed', 'failed'],
    ['unexpected', 'status unknown'],
  ]
  const historyWith = (toolActivity, phase = 'complete') => [{
    runId: 'run-tools', phase, text: 'I used tools.', prompt: 'Do work', receipt: {}, toolActivity,
  }]

  function restore(toolActivity, phase) {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return historyWith(toolActivity, phase)
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    return render(App)
  }

  it.each(statuses)('matches visible and accessible %s status text in a single card', async (status, label) => {
    restore([{ effectId: 'tool-1', displayName: 'Search files', status }])

    const row = await screen.findByRole('listitem', { name: `Search files ${label}` })
    const visibleText = [...row.querySelectorAll('.tool-name, .tool-status')].map((part) => part.textContent).join(' ')
    expect(visibleText).toBe(row.getAttribute('aria-label'))
  })

  it('renders three sequential tools as one named list without status roles', async () => {
    restore([
      { effectId: 'tool-1', displayName: 'Search files', status: 'completed' },
      { effectId: 'tool-2', displayName: 'Read file', status: 'completed' },
      { effectId: 'tool-3', displayName: 'Summarize file', status: 'completed' },
    ])

    const list = await screen.findByRole('list', { name: 'Tool activity' })
    expect(within(list).getAllByRole('listitem')).toHaveLength(3)
    expect(document.querySelectorAll('[role="status"]')).toHaveLength(0)
  })

  it('updates a live running card to completed', async () => {
    restore([{ effectId: 'tool-1', displayName: 'Read file', status: 'running' }], 'streaming')
    expect(await screen.findByRole('listitem', { name: 'Read file running' })).toBeInTheDocument()
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))

    chatListener({ payload: {
      runId: 'run-tools', phase: 'complete', text: 'I used tools.', receipt: {},
      toolActivity: [{ effectId: 'tool-1', displayName: 'Read file', status: 'completed' }],
    } })

    expect(await screen.findByRole('listitem', { name: 'Read file completed' })).toBeInTheDocument()
    expect(screen.queryByRole('listitem', { name: 'Read file running' })).not.toBeInTheDocument()
  })

  it('labels failed activity with text and supplies a neutral missing name', async () => {
    restore([{ effectId: 'tool-1', displayName: null, status: 'failed' }])

    const card = await screen.findByRole('listitem', { name: 'Tool activity failed' })
    expect(card).toHaveTextContent('Tool activity')
    expect(card).toHaveTextContent('failed')
  })

  it('renders the failed status as static oxide text without generated content or fill', () => {
    expect(appRules.has('.tool-failed .tool-status::before')).toBe(false)
    expect(appRules.get('.tool-failed .tool-status')).toMatch(/color:\s*var\(--oxide\)/)
    expect(appRules.get('.tool-failed .tool-status')).not.toMatch(/\b(?:animation|background|content)\s*:/)
  })

  it.each(statuses)('matches visible and accessible %s status text in a parallel group', async (status, label) => {
    restore([
      { effectId: 'tool-1', displayName: 'Search files', status: 'running' },
      { effectId: 'tool-2', displayName: 'Read file', status: 'running' },
    ], 'streaming')
    await screen.findByRole('group', { name: 'Parallel tool activity: Search files running, Read file running' })

    if (status !== 'running') {
      await waitFor(() => expect(chatListener).toBeTypeOf('function'))
      chatListener({ payload: {
        runId: 'run-tools', phase: 'complete', text: 'I used tools.', receipt: {},
        toolActivity: [
          { effectId: 'tool-1', displayName: 'Search files', status },
          { effectId: 'tool-2', displayName: 'Read file', status },
        ],
      } })
    }

    const group = await screen.findByRole('group', { name: `Parallel tool activity: Search files ${label}, Read file ${label}` })
    for (const name of ['Search files', 'Read file']) {
      const row = within(group).getByRole('listitem', { name: `${name} ${label}` })
      const visibleText = [...row.querySelectorAll('.tool-name, .tool-status')].map((part) => part.textContent).join(' ')
      expect(visibleText).toBe(row.getAttribute('aria-label'))
    }
  })

  it('groups parallel running effects and keeps every status row visible when settled', async () => {
    restore([
      { effectId: 'tool-1', displayName: 'Search files', status: 'running' },
      { effectId: 'tool-2', displayName: 'Read file', status: 'running' },
    ], 'streaming')

    const group = await screen.findByRole('group', { name: /Parallel tool activity: Search files running, Read file running/ })
    expect(within(group).getByRole('listitem', { name: 'Search files running' })).toBeInTheDocument()
    expect(within(group).getByRole('listitem', { name: 'Read file running' })).toBeInTheDocument()
    await waitFor(() => expect(chatListener).toBeTypeOf('function'))

    chatListener({ payload: {
      runId: 'run-tools', phase: 'complete', text: 'I used tools.', receipt: {},
      toolActivity: [
        { effectId: 'tool-1', displayName: 'Search files', status: 'completed' },
        { effectId: 'tool-2', displayName: 'Read file', status: 'failed' },
      ],
    } })

    const settled = await screen.findByRole('group', { name: /Search files completed, Read file failed/ })
    expect(within(settled).getByRole('listitem', { name: 'Search files completed' })).toBeInTheDocument()
    expect(within(settled).getByRole('listitem', { name: 'Read file failed' })).toBeInTheDocument()
  })

  it('clears stale parallel grouping when history is reloaded', async () => {
    let signedIn = true
    let historyLoads = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: signedIn, subject: signedIn ? 'user-a' : null }
      if (command === 'auth_sign_out') {
        signedIn = false
        return { signed_in: false, subject: null }
      }
      if (command === 'auth_sign_in') {
        signedIn = true
        return { signed_in: true, subject: 'user-a' }
      }
      if (command === 'chat_thread_open') {
        historyLoads += 1
        return historyLoads === 1
          ? historyWith([
              { effectId: 'tool-1', displayName: 'Search files', status: 'running' },
              { effectId: 'tool-2', displayName: 'Read file', status: 'running' },
            ], 'streaming')
          : historyWith([
              { effectId: 'tool-1', displayName: 'Search files', status: 'completed' },
            ])
      }
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    expect(await screen.findByRole('group', { name: /Parallel tool activity/ })).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: /Access unavailable|Alice/ }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign out' }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in' }))

    expect(await screen.findByRole('listitem', { name: 'Search files completed' })).toBeInTheDocument()
    expect(screen.queryByRole('group', { name: /Parallel tool activity/ })).not.toBeInTheDocument()
  })
})

describe('message action row', () => {
  const reply = (overrides = {}) => ({
    runId: 'run-copy', phase: 'complete', text: 'A routed answer', prompt: 'A question',
    receipt: {}, toolActivity: [], ...overrides,
  })

  function restore(history = [reply()]) {
    invoke.mockImplementation(async (command) => {
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

  it('offers copy alone: no fork, share, or retry on a settled reply', async () => {
    restore()

    await screen.findByRole('button', { name: 'Copy' })
    expect(document.querySelectorAll('.message-actions button')).toHaveLength(1)
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return [{ runId: 'run-receipt', phase: 'complete', text: 'A routed answer', prompt: 'A question', receipt, recalls, toolActivity: [] }]
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      throw new Error(`unexpected command: ${command}`)
    })
    return render(App)
  }

  it('renders route → model · cost · time with signal on the route segment', async () => {
    restore({ route: 'analysis/high', model: 'glm-5.2', cost: '$0.0089', time: '6.2s' })

    const line = await screen.findByRole('button', { name: 'Expand receipt: Routed via analysis/high to model glm-5.2, $0.0089, 6.2s' })
    expect(line.textContent).toBe('analysis/high → glm-5.2 · $0.0089 · 6.2s')
    expect(within(line).getByText('analysis/high')).toHaveClass('route-segment')
    expect(line.querySelectorAll('.route-segment')).toHaveLength(1)
  })

  it('paints nothing green when the receipt records no route', async () => {
    restore({ model: 'glm-5.2', cost: '$0.0089', time: '6.2s' })

    const line = await screen.findByRole('button', { name: 'Expand receipt: Model glm-5.2, $0.0089, 6.2s' })
    expect(line.textContent).toBe('glm-5.2 · $0.0089 · 6.2s')
    expect(line.querySelector('.route-segment')).toBeNull()
  })

  it('renders a route-only receipt without a dangling arrow', async () => {
    restore({ route: 'analysis/high' })

    const line = await screen.findByRole('button', { name: 'Expand receipt: Routed via analysis/high' })
    expect(line.textContent).toBe('analysis/high')
  })

  it('expands to the receipt record and back', async () => {
    restore({ route: 'analysis/high', model: 'glm-5.2', cost: '$0.0089', time: '6.2s', capabilities: [{ name: 'search', version: '2' }] })

    const line = await screen.findByRole('button', { name: /^Expand receipt:/ })
    const marker = line.querySelector('.receipt-marker')
    expect(line.textContent).toBe('analysis/high → glm-5.2 · $0.0089 · 6.2s · search@2')
    expect(marker).toHaveAttribute('aria-hidden', 'true')
    expect(marker).not.toHaveClass('expanded')
    await fireEvent.click(line)

    const record = document.querySelector('.receipt-record')
    expect(record.textContent).toBe('Routeanalysis/highModelglm-5.2Cost$0.0089Time6.2sCapabilitysearch@2')
    expect(record.querySelectorAll('.route-value')).toHaveLength(1)
    expect(await screen.findByRole('button', { name: 'Collapse receipt: Routed via analysis/high to model glm-5.2, $0.0089, 6.2s, search@2' })).toBe(line)
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
    expect(record.textContent).toBe('Routeanalysis/highModelglm-5.2Cost$0.0089Time6.2sMemorylease · 2 filesDocuments/Muniment/lease.pdfDocuments/Muniment/notes.mdMemorymissing clause · 0 files')
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
    const send = screen.getByRole('button', { name: 'Send' })
    expect(send).toHaveClass('primary')
    await fireEvent.click(send)

    expectQueuePayload({ runId: 'run-7', delivery: 'steer', message: 'Focus on the risks' })
  })

  it('keeps focus on the same Send button when a run starts', async () => {
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: 'Initial prompt' } })
    const send = screen.getByRole('button', { name: 'Send' })
    send.focus()

    await fireEvent.click(send)

    expect(document.activeElement).toBe(send)
    expect(screen.getByRole('button', { name: 'Send' })).toBe(send)
    expect(send).toHaveAttribute('aria-disabled', 'true')
  })

  it('shows a queue rejection and preserves the draft', async () => {
    invoke.mockImplementation(async (command) => {
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
    await fireEvent.click(screen.getByRole('button', { name: 'Queue follow-up' }))

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
    const composer = await screen.findByPlaceholderText('Ask anything')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'chat_submit') return { runId: 'run-9', attachments: [] }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'dictation_start') return { state: 'running' }
      if (command === 'dictation_stop') return { state: 'stopped' }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    const composer = await screen.findByPlaceholderText('Ask anything')
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
    const composer = await screen.findByPlaceholderText('Ask anything')
    expect(composer.style.height).toBe(resting)

    // Retry writes the four-line prompt into the draft with no input event,
    // then send() clears it once the submission lands.
    await fireEvent.click(await screen.findByRole('button', { name: 'Try again' }))
    await waitFor(() => expect(composer.style.height).toBe(`${4 * row}px`))

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
    const composer = await screen.findByPlaceholderText('Ask anything')
    await fireEvent.input(composer, { target: { value: lines(6) } })
    expect(composer.style.height).toBe(`${6 * row}px`)

    // Opening the artifact rail narrows the composer without touching the
    // window: the same draft rewraps onto eight lines. Left unmeasured the box
    // stays six rows tall with overflow hidden, so two lines of the user's own
    // draft would be invisible and unreachable.
    rendered = (value) => value.split('\n').length + 2
    expect(observers).toHaveLength(1)
    // The action row, never the input: observing a box this callback resizes
    // makes the browser report an undelivered ResizeObserver loop on every drag.
    expect(observers[0].target).toHaveClass('composer-row')
    observers[0].callback()
    expect(composer.style.height).toBe(`${8 * row}px`)
    expect(composer.style.overflowY).toBe('hidden')

    cleanup()
    expect(observers[0].disconnected).toBe(true)
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
      if (command === 'chat_thread_open') return []
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
      if (command === 'attach_companions') return []
      if (command === 'attach_listener_status') return { started: true, failure: null }
      throw new Error(`unexpected command: ${command}`)
    })

    render(App)
    const profile = await screen.findByRole('button', { name: /Alice/i })
    await fireEvent.click(profile)

    const dialog = screen.getByRole('dialog', { name: 'Profile' })
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
    expect(screen.queryByRole('dialog', { name: 'Profile' })).not.toBeInTheDocument()
    expect(profile).toHaveFocus()
    expect(profile).toHaveAttribute('aria-expanded', 'false')

    await fireEvent.click(profile)
    await screen.findByRole('dialog', { name: 'Profile' })
    await fireEvent.click(document.body)
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Profile' })).not.toBeInTheDocument())
    expect(profile).toHaveFocus()
  })

  it('renders mixed device states in deterministic order without disturbing groups', async () => {
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
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
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
    const rows = await within(dialog).findAllByRole('listitem')
    expect(rows.map((row) => row.textContent)).toEqual(expect.arrayContaining([
      expect.stringContaining('This device'), expect.stringContaining('Active'), expect.stringContaining('Revoked'),
    ]))
    expect(rows[0]).toHaveTextContent('DesktopThis deviceActive')
    expect(rows[1]).toHaveTextContent('AndroidActive')
    expect(rows[2]).toHaveTextContent('iOSRevoked')
    expect(rows[2]).toHaveClass('revoked')
    const activeIdentifier = within(rows[1]).getByText('Android')
    const revokedIdentifier = within(rows[2]).getByText('iOS')
    const revokedRule = accessPanelSource.match(/\.revoked \.device-heading strong\s*\{([^}]*)\}/)?.[1]
    expect(revokedIdentifier.tagName).toBe('STRONG')
    expect(activeIdentifier.tagName).toBe('STRONG')
    expect(revokedRule).toMatch(/color:\s*var\(--oxide\)/)
    expect(revokedRule).toMatch(/text-decoration:\s*line-through/)
    expect(revokedRule).toMatch(/font-weight:\s*400/)
    expect(within(rows[2]).getByText('Revoked')).toBeVisible()
    expect(within(dialog).getByRole('button', { name: 'members' })).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('revoked-newest')
  })

  it('renders an empty device response', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    expect(await screen.findByText('No devices found')).toBeInTheDocument()
  })

  it('applies and persists an accessible per-device appearance choice', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const appearance = within(screen.getByRole('group', { name: 'Appearance' }))
    const system = appearance.getByRole('button', { name: 'System' })
    const dark = appearance.getByRole('button', { name: 'Dark' })

    expect(system).toHaveAttribute('aria-pressed', 'true')
    expect(dark).toHaveAttribute('aria-pressed', 'false')

    await fireEvent.click(dark)
    expect(document.documentElement.dataset.theme).toBe('dark')
    expect(localStorage.getItem('muniment.theme')).toBe('dark')
    expect(dark).toHaveAttribute('aria-pressed', 'true')

    await fireEvent.click(system)
    expect(document.documentElement).not.toHaveAttribute('data-theme')
    expect(localStorage.getItem('muniment.theme')).toBe('system')
    expect(system).toHaveAttribute('aria-pressed', 'true')
  })

  it('keeps the primary action fixed and orders the scrolling profile sections', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
    const content = dialog.querySelector('.access-content')
    const signOut = within(dialog).getByRole('button', { name: 'Sign out' })

    expect(content).toBeInTheDocument()
    expect(content).not.toContainElement(signOut)
    expect(signOut.closest('.access-footer')).toBeInTheDocument()
    expect([...content.querySelectorAll(':scope > section')].map((section) => section.getAttribute('aria-labelledby'))).toEqual([
      'appearance-heading',
      'entitlements-heading',
      'devices-heading',
      'companions-heading',
      'voice-heading',
    ])
    expect(content.querySelector('.entitlements-section')).toHaveTextContent('Access is set by your admins.')
  })

  it('identifies the profile and keeps access snapshot metadata in its section', async () => {
    render(App)
    const profile = await screen.findByRole('button', { name: /Alice/i })
    expect(profile).toHaveAttribute('title', 'Acme · owner')
    await fireEvent.click(profile)

    const dialog = screen.getByRole('dialog', { name: 'Profile' })
    const header = dialog.querySelector(':scope > header')
    const accessSection = dialog.querySelector('.entitlements-section')
    expect(header).toHaveTextContent('Alice')
    expect(header).toHaveTextContent('Acme · owner')
    expect(within(dialog).getByRole('button', { name: 'Close profile' })).toBeInTheDocument()
    expect(within(dialog).getAllByRole('heading', { name: 'Your access' })).toHaveLength(1)
    expect(header).not.toHaveTextContent('Snapshot')
    expect(accessSection).toHaveTextContent('Snapshot v2')
    expect(dialog).not.toHaveTextContent('Your groups')
  })

  it('retries only a failed device request and keeps entitlement groups rendered', async () => {
    let deviceCalls = 0
    invoke.mockImplementation(async (command) => {
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot([{ name: 'members', models: [], connections: [], capabilities: [] }])
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
    expect(await within(dialog).findByText('Devices could not be loaded.')).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('raw backend secret')
    expect(within(dialog).getByRole('button', { name: 'members' })).toBeInTheDocument()
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
      if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
      if (command === 'chat_thread_open') return []
      if (command === 'auth_entitlement_snapshot') return snapshot()
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return pendingCompanions.promise
      if (command === 'attach_listener_status') return { started: true, failure: null }
      throw new Error(`unexpected command: ${command}`)
    })
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const dialog = screen.getByRole('dialog', { name: 'Profile' })
    expect(within(dialog).getByText('Loading connected programs…')).toBeInTheDocument()

    pendingCompanions.resolve([{ identity: 'client-1', claimed_kind: longKind, claimed_version: longVersion, approved_at: null }])
    const kind = await within(dialog).findByText(longKind)
    expect(kind).toHaveAttribute('title', longKind)
    expect(within(dialog).getByText('Claimed kind')).toBeInTheDocument()
    expect(within(dialog).getByText(`Claimed version: ${longVersion}`)).toHaveAttribute('title', longVersion)
    expect(within(dialog).getByText('Approval time unavailable')).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('client-1')
  })

  it('renders an empty connected program response', async () => {
    render(App)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    expect(await screen.findByText('No connected programs found')).toBeInTheDocument()
  })

  it('retries only a failed connected program request', async () => {
    let companionCalls = 0
    invoke.mockImplementation(async (command) => {
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
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/i }))
    const section = screen.getByRole('heading', { name: 'Connected programs' }).closest('section')
    expect(await within(section).findByText('Connected programs could not be loaded.')).toBeInTheDocument()
    expect(section).not.toHaveTextContent('raw backend secret')
    await fireEvent.click(within(section).getByRole('button', { name: 'Try again' }))
    expect(await within(section).findByText('ACP adapter')).toBeInTheDocument()
    expect(companionCalls).toBe(2)
  })
})

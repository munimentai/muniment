// @vitest-environment jsdom

import fs from 'node:fs'
import path from 'node:path'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import AccessPanel from './AccessPanel.svelte'

let closeWindow

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ close: (...args) => closeWindow(...args) }),
}))

const source = fs.readFileSync(path.join(process.cwd(), 'src/lib/AccessPanel.svelte'), 'utf8')
const styles = source.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const rules = new Map([...styles
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))

const snapshot = {
  snapshot_version: 2,
  user_display_name: 'Alice',
  organization_display_name: 'Acme',
  org_id: 'acme',
  role: 'owner',
  groups: [],
}
const companion = { identity: 'client-1', claimed_kind: 'cli', claimed_version: '1.2.3', approved_at: '2026-08-04T12:00:00Z' }

function renderPanel(invoke) {
  return render(AccessPanel, { props: {
    tauri: { invoke },
    subject: 'user-1',
    onSignOut: vi.fn(),
    voiceShortcut: 'Control+Space',
    voiceShortcutChanging: false,
    onVoiceShortcutChange: vi.fn(),
    defaultVoiceShortcut: 'Control+Space',
  } })
}

function companionInvoke(revoke = async () => {}) {
  return vi.fn(async (command, args) => {
    if (command === 'auth_entitlement_snapshot') return snapshot
    if (command === 'auth_devices') return []
    if (command === 'attach_companions') return [companion]
    if (command === 'attach_listener_status') return { started: true, failure: null }
    if (command === 'attach_revoke_companion') return revoke(args)
    throw new Error(`unexpected command: ${command}`)
  })
}

beforeEach(() => {
  vi.stubGlobal('requestAnimationFrame', (callback) => callback())
  closeWindow = vi.fn()
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('access popover layout', () => {
  it('bounds the column while only its content region scrolls', () => {
    expect(rules.get('.access-popover')).toMatch(/max-height:\s*min\(\d+px,\s*70vh\)/)
    expect(rules.get('.access-popover')).toMatch(/display:\s*flex/)
    expect(rules.get('.access-popover')).toMatch(/flex-direction:\s*column/)
    expect(rules.get('.access-popover')).not.toMatch(/overflow-y\s*:/)
    expect(rules.get('.access-content')).toMatch(/overflow-y:\s*auto/)
  })

  it('separates the fixed header and footer from overflowing content', () => {
    expect(rules.get('.access-popover header')).toMatch(/flex:\s*none/)
    expect(rules.get('.access-popover header')).toMatch(/border-bottom:\s*1px solid var\(--border\)/)
    expect(rules.get('.access-footer')).toMatch(/flex:\s*none/)
    expect(rules.get('.access-footer')).toMatch(/border-top:\s*1px solid var\(--border\)/)
  })

  it('keeps profile metadata on one readable line', () => {
    expect(rules.get('.profile-button small')).toMatch(/overflow:\s*hidden/)
    expect(rules.get('.profile-button small')).toMatch(/text-overflow:\s*ellipsis/)
    expect(rules.get('.profile-button small')).toMatch(/white-space:\s*nowrap/)
    expect(source).toMatch(/class="profile-button" title=\{profileDetails\}/)
  })

  it('renders every connected program state and a missing approval time', () => {
    expect(source).toMatch(/Loading connected programs…/)
    expect(source).toMatch(/Connected programs could not be loaded\./)
    expect(source).toMatch(/No connected programs found/)
    expect(source).toMatch(/The connected programs folder is unavailable\./)
    expect(source).toMatch(/Connected programs are available in another Muniment window\./)
    expect(source).toMatch(/The connected programs connection could not start\./)
    expect(source).toMatch(/The connected programs listener stopped\./)
    expect(source).toMatch(/Restart Muniment/)
    expect(source).toMatch(/Close this window/)
    expect(source).toMatch(/Approval time unavailable/)
    expect(source).toMatch(/onclick=\{loadCompanions\}>Try again/)
  })

  it('keeps claimed program text on one line with its full value available', () => {
    expect(source).toMatch(/<strong title=\{companion\.claimed_kind\}>/)
    expect(source).toMatch(/class="companion-version" title=\{companion\.claimed_version\}/)
    expect(rules.get('.companion-heading strong, .companion-version')).toMatch(/text-overflow:\s*ellipsis/)
    expect(rules.get('.companion-heading strong, .companion-version')).toMatch(/white-space:\s*nowrap/)
  })

  it('reveals a tabbed revoke control through row focus', () => {
    expect(rules.get('.companion-revoke')).toMatch(/opacity:\s*0/)
    expect(rules.get('.companion-row:hover .companion-revoke, .companion-row:focus-within .companion-revoke')).toMatch(/opacity:\s*1/)
    expect(source).not.toMatch(/class="companion-revoke"[^>]*tabindex="-1"/)
  })

  it('cancels a revoke with Escape and returns focus without closing the popover', async () => {
    renderPanel(companionInvoke())
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))
    const revoke = await screen.findByRole('button', { name: 'Revoke cli' })

    await fireEvent.click(revoke)
    const confirm = screen.getByRole('group', { name: 'Revoke cli?' })
    expect(confirm).toHaveTextContent('must be approved again before it can reconnect')
    await fireEvent.keyDown(within(confirm).getByRole('button', { name: 'Cancel' }), { key: 'Escape' })

    const restored = await screen.findByRole('button', { name: 'Revoke cli' })
    await waitFor(() => expect(restored).toHaveFocus())
    expect(screen.getByRole('dialog', { name: 'Profile' })).toBeInTheDocument()
  })

  it('keeps the profile open when document receives Escape during a revoke', async () => {
    renderPanel(companionInvoke())
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Revoke cli' }))

    await fireEvent.keyDown(document, { key: 'Escape' })

    const restored = await screen.findByRole('button', { name: 'Revoke cli' })
    await waitFor(() => expect(restored).toHaveFocus())
    expect(screen.getByRole('dialog', { name: 'Profile' })).toBeInTheDocument()
  })

  it('reloads programs after a confirmed revoke', async () => {
    let programs = [companion]
    const invoke = vi.fn(async (command, args) => {
      if (command === 'auth_entitlement_snapshot') return snapshot
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return programs
      if (command === 'attach_listener_status') return { started: true, failure: null }
      if (command === 'attach_revoke_companion') {
        expect(args).toEqual({ clientIdentity: companion.identity })
        programs = []
        return
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Revoke cli' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))

    expect(await screen.findByText('No connected programs found')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('attach_revoke_companion', { clientIdentity: companion.identity })
    expect(invoke.mock.calls.filter(([command]) => command === 'attach_companions')).toHaveLength(2)
  })

  for (const [failure, message, recovery] of [
    ['filesystem', 'The connected programs folder is unavailable.', 'Restart Muniment'],
    ['instance_lock', 'Connected programs are available in another Muniment window.', 'Close this window'],
    ['bind', 'The connected programs connection could not start.', 'Restart Muniment'],
  ]) {
    it(`renders the ${failure} listener failure and its recovery`, async () => {
      const invoke = vi.fn(async (command) => {
        if (command === 'auth_entitlement_snapshot') return snapshot
        if (command === 'auth_devices') return []
        if (command === 'attach_companions') return []
        if (command === 'attach_listener_status') return { started: false, failure }
        if (command === 'restart_muniment') return
        throw new Error(`unexpected command: ${command}`)
      })
      renderPanel(invoke)
      await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))

      expect(await screen.findByText(message)).toBeInTheDocument()
      await fireEvent.click(screen.getByRole('button', { name: recovery }))
      if (failure === 'instance_lock') expect(closeWindow).toHaveBeenCalledOnce()
      else expect(invoke).toHaveBeenCalledWith('restart_muniment')
      expect(screen.queryByText('No connected programs found')).not.toBeInTheDocument()
    })
  }

  it('renders the stopped listener and its recovery', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_entitlement_snapshot') return snapshot
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return []
      if (command === 'attach_listener_status') return { started: false, failure: null, pending: false, stopped: true }
      if (command === 'restart_muniment') return
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))

    expect(await screen.findByText('The connected programs listener stopped.')).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Restart Muniment' }))
    expect(invoke).toHaveBeenCalledWith('restart_muniment')
    expect(screen.queryByText('No connected programs found')).not.toBeInTheDocument()
  })

  it('keeps loading while listener startup is pending', async () => {
    let statusCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_entitlement_snapshot') return snapshot
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return []
      if (command === 'attach_listener_status') {
        statusCalls += 1
        return statusCalls === 1
          ? { started: false, failure: null, pending: true }
          : { started: true, failure: null, pending: false }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))

    expect(await screen.findByText('Loading connected programs…')).toBeInTheDocument()
    expect(screen.queryByText('No connected programs found')).not.toBeInTheDocument()
    expect(await screen.findByText('No connected programs found')).toBeInTheDocument()
  })

  it('stops polling when listener initialization fails', async () => {
    let statusCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_entitlement_snapshot') return snapshot
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return []
      if (command === 'attach_listener_status') {
        statusCalls += 1
        return statusCalls === 1
          ? { started: false, failure: null, pending: true }
          : { started: false, failure: 'filesystem', pending: false }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))

    expect(await screen.findByText('Loading connected programs…')).toBeInTheDocument()
    expect(await screen.findByText('The connected programs folder is unavailable.')).toBeInTheDocument()
    expect(statusCalls).toBe(2)
    expect(screen.queryByText('No connected programs found')).not.toBeInTheDocument()
  })

  it('renders the empty list when the listener started', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_entitlement_snapshot') return snapshot
      if (command === 'auth_devices') return []
      if (command === 'attach_companions') return []
      if (command === 'attach_listener_status') return { started: true, failure: null }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))

    expect(await screen.findByText('No connected programs found')).toBeInTheDocument()
  })

  it('keeps a failed revoke in its row', async () => {
    const invoke = companionInvoke(async () => { throw new Error('failed') })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Revoke cli' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('The program could not be revoked.')
    expect(screen.getByRole('button', { name: 'Retry revoke' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument()
    expect(screen.getByRole('group', { name: 'Revoke cli?' })).toBeInTheDocument()
  })

  it('keeps the confirm visible when Escape precedes a failed revoke', async () => {
    let rejectRevoke
    const pendingRevoke = new Promise((_, reject) => { rejectRevoke = reject })
    renderPanel(companionInvoke(() => pendingRevoke))
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))
    await fireEvent.click(await screen.findByRole('button', { name: 'Revoke cli' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))

    await fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.getByRole('group', { name: 'Revoke cli?' })).toBeInTheDocument()
    rejectRevoke(new Error('failed'))

    expect(await screen.findByRole('alert')).toHaveTextContent('The program could not be revoked.')
    expect(screen.getByRole('group', { name: 'Revoke cli?' })).toBeInTheDocument()
  })
})

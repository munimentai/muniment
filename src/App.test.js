// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

let App
let invoke

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

beforeAll(async () => {
  HTMLElement.prototype.scrollTo = vi.fn()
  window.__TAURI__ = {
    core: { invoke: (...args) => invoke(...args) },
    event: { listen: vi.fn().mockResolvedValue(vi.fn()) },
  }
  App = (await import('./App.svelte')).default
})

beforeEach(() => {
  invoke = vi.fn(async (command) => {
    if (command === 'auth_status') return { signed_in: true, subject: 'token-subject' }
    if (command === 'chat_history') return []
    if (command === 'auth_entitlement_snapshot') return snapshot()
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => cleanup())

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
})

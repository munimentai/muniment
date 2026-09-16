// @vitest-environment jsdom

import fs from 'node:fs'
import path from 'node:path'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it, vi } from 'vitest'

import AccessPanel from './AccessPanel.svelte'

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
  capabilities: [],
  grants: [],
}

function renderPanel(invoke = vi.fn(async () => snapshot), onSignOut = vi.fn()) {
  render(AccessPanel, { props: { tauri: { invoke }, subject: 'user-1', onSignOut } })
  return { invoke, onSignOut }
}

afterEach(() => cleanup())

describe('signed-in profile menu', () => {
  it('names the person and their organization from the server snapshot', async () => {
    renderPanel()
    const profile = await screen.findByRole('button', { name: /Alice/ })
    expect(profile).toHaveTextContent('Acme · owner')
    expect(profile).toHaveAttribute('aria-expanded', 'false')
    expect(profile).not.toHaveAttribute('title')
  })

  it('falls back to the subject when the snapshot cannot be read', async () => {
    renderPanel(vi.fn(async () => { throw new Error('offline') }))
    expect(await screen.findByRole('button', { name: /user-1/ })).toHaveTextContent('Access unavailable')
  })

  it('offers sign out alone and reports the choice once', async () => {
    const { onSignOut } = renderPanel()
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))

    const menu = screen.getByRole('menu', { name: 'Account' })
    expect(menu.querySelectorAll('button')).toHaveLength(1)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Sign out' }))

    expect(onSignOut).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('menu', { name: 'Account' })).not.toBeInTheDocument()
  })

  it('closes on Escape and returns focus to the profile', async () => {
    renderPanel()
    const profile = await screen.findByRole('button', { name: /Alice/ })
    await fireEvent.click(profile)

    await fireEvent.keyDown(document, { key: 'Escape' })

    await waitFor(() => expect(screen.queryByRole('menu', { name: 'Account' })).not.toBeInTheDocument())
    expect(profile).toHaveFocus()
  })

  it('closes when a click lands outside the menu', async () => {
    renderPanel()
    await fireEvent.click(await screen.findByRole('button', { name: /Alice/ }))

    await fireEvent.click(document.body)

    await waitFor(() => expect(screen.queryByRole('menu', { name: 'Account' })).not.toBeInTheDocument())
  })

  it('pads the menu and its item like the shell’s other menus', () => {
    expect(rules.get('.profile-menu')).toMatch(/padding:\s*4px/)
    expect(rules.get('.profile-menu button')).toMatch(/padding:\s*3px 8px/)
    expect(rules.get('.profile-menu button')).toMatch(/min-height:\s*24px/)
    expect(rules.get('.profile-menu button')).toMatch(/text-align:\s*left/)
  })

  it('retries a failed snapshot and reads it again when the menu opens', async () => {
    let calls = 0
    const invoke = vi.fn(async () => {
      calls += 1
      if (calls === 1) throw new Error('not connected yet')
      return snapshot
    })
    renderPanel(invoke)

    expect(await screen.findByRole('button', { name: /user-1/ })).toHaveTextContent('Access unavailable')
    await waitFor(() => expect(screen.getByRole('button', { name: /Alice/ })).toBeInTheDocument(), { timeout: 3000 })

    await fireEvent.click(screen.getByRole('button', { name: /Alice/ }))
    await waitFor(() => expect(calls).toBeGreaterThanOrEqual(3))
  })

  it('keeps profile metadata on one readable line', () => {
    expect(rules.get('.profile-button small')).toMatch(/overflow:\s*hidden/)
    expect(rules.get('.profile-button small')).toMatch(/text-overflow:\s*ellipsis/)
    expect(rules.get('.profile-button small')).toMatch(/white-space:\s*nowrap/)
  })
})

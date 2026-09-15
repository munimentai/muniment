import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'

import Appearance from './Appearance.svelte'

afterEach(() => {
  cleanup()
  localStorage.clear()
  delete document.documentElement.dataset.theme
  delete document.documentElement.dataset.scheme
})

const stored = () => JSON.parse(localStorage.getItem('muniment.theme'))
const pressed = (group) => within(group).getAllByRole('button').filter((button) => button.getAttribute('aria-pressed') === 'true').map((button) => button.textContent.trim())

describe('Appearance', () => {
  it('offers three modes and eight themes, each with a swatch of its own paper and surface', () => {
    render(Appearance)
    const modes = screen.getByRole('group', { name: 'Appearance' })
    expect(within(modes).getAllByRole('button').map((button) => button.textContent)).toEqual(['System', 'Light', 'Dark'])
    const themes = screen.getByRole('group', { name: 'Themes' })
    expect(within(themes).getAllByRole('button').map((button) => button.textContent.trim())).toEqual(['Paper', 'Vellum', 'Ledger', 'Foolscap', 'Moss', 'Vault', 'Graphite', 'Inkwell'])
    expect([...themes.querySelectorAll('[data-swatch]')].map((swatch) => swatch.dataset.theme)).toEqual(['paper', 'vellum', 'ledger', 'foolscap', 'moss', 'vault', 'graphite', 'inkwell'])
    expect(pressed(modes)).toEqual(['System'])
    expect(pressed(themes)).toEqual(['Paper', 'Vault'])
  })

  it('picks a theme with its mode, keeps the last pick per mode, and follows the OS in System', async () => {
    render(Appearance)
    const modes = screen.getByRole('group', { name: 'Appearance' })
    const themes = screen.getByRole('group', { name: 'Themes' })
    await fireEvent.click(within(themes).getByRole('button', { name: 'Vault' }))
    expect(document.documentElement.dataset).toMatchObject({ theme: 'vault', scheme: 'dark' })
    expect(stored()).toEqual({ mode: 'dark', light: 'paper', dark: 'vault' })
    expect(pressed(modes)).toEqual(['Dark'])
    expect(pressed(themes)).toEqual(['Vault'])
    await fireEvent.click(within(themes).getByRole('button', { name: 'Ledger' }))
    expect(document.documentElement.dataset).toMatchObject({ theme: 'ledger', scheme: 'light' })
    expect(stored()).toEqual({ mode: 'light', light: 'ledger', dark: 'vault' })
    await fireEvent.click(within(modes).getByRole('button', { name: 'Dark' }))
    expect(document.documentElement.dataset.theme).toBe('vault')
    expect(pressed(themes)).toEqual(['Vault'])
    await fireEvent.click(within(modes).getByRole('button', { name: 'Light' }))
    expect(document.documentElement.dataset.theme).toBe('ledger')
    await fireEvent.click(within(modes).getByRole('button', { name: 'System' }))
    expect(document.documentElement.dataset.theme).toBeUndefined()
    expect(document.documentElement.dataset.scheme).toBeUndefined()
    expect(stored()).toEqual({ mode: 'system', light: 'ledger', dark: 'vault' })
    expect(pressed(themes)).toEqual(['Paper', 'Vault'])
  })
})

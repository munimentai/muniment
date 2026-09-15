import { describe, expect, it } from 'vitest'

import {
  DARK_THEMES,
  LIGHT_THEMES,
  THEME_DARK,
  THEME_LIGHT,
  THEME_STORAGE_KEY,
  THEME_SYSTEM,
  activeTheme,
  applyTheme,
  parseTheme,
  serializeTheme,
  themeScheme,
} from './theme-state.js'

const system = { mode: THEME_SYSTEM, light: 'paper', dark: 'vault' }

describe('theme state', () => {
  it('names four light and four dark themes with a scheme each', () => {
    expect(LIGHT_THEMES).toEqual(['Paper', 'Vellum', 'Ledger', 'Foolscap', 'Parchment', 'Manila', 'Linen', 'Broadsheet'].map((name) => name.toLowerCase()))
    expect(DARK_THEMES).toEqual(['Moss', 'Vault', 'Graphite', 'Inkwell', 'Lagoon', 'Umber', 'Fjord', 'Plum', 'Nocturne', 'Nightshade', 'Basalt', 'Obsidian', 'Carbon'].map((name) => name.toLowerCase()))
    for (const theme of LIGHT_THEMES) expect(themeScheme(theme)).toBe(THEME_LIGHT)
    for (const theme of DARK_THEMES) expect(themeScheme(theme)).toBe(THEME_DARK)
    expect(themeScheme('system')).toBe(null)
  })

  it('round-trips each state and defaults missing or malformed data to system', () => {
    expect(THEME_STORAGE_KEY).toBe('muniment.theme')
    for (const state of [system, { mode: 'light', light: 'ledger', dark: 'vault' }, { mode: 'dark', light: 'paper', dark: 'inkwell' }]) {
      expect(parseTheme(serializeTheme(state))).toEqual(state)
    }
    expect(parseTheme(null)).toEqual(system)
    expect(parseTheme(undefined)).toEqual(system)
    expect(parseTheme('')).toEqual(system)
    expect(parseTheme('Dark')).toEqual(system)
    expect(parseTheme('{"theme":"dark"}')).toEqual(system)
    expect(parseTheme('{"mode":"dark","light":"neon","dark":"neon"}')).toEqual({ ...system, mode: 'dark' })
    expect(parseTheme('[1]')).toEqual(system)
    expect(serializeTheme('invalid')).toBe(JSON.stringify(system))
  })

  it('reads the three older bare modes and a bare theme name', () => {
    expect(parseTheme('system')).toEqual(system)
    expect(parseTheme('light')).toEqual({ ...system, mode: 'light' })
    expect(parseTheme('dark')).toEqual({ ...system, mode: 'dark' })
    expect(parseTheme('moss')).toEqual({ mode: 'dark', light: 'paper', dark: 'moss' })
    expect(parseTheme('vellum')).toEqual({ mode: 'light', light: 'vellum', dark: 'vault' })
  })

  it('applies the picked theme and its scheme to the root, or nothing in system', () => {
    const root = { dataset: {} }
    applyTheme(root, { mode: 'dark', light: 'ledger', dark: 'vault' })
    expect(root.dataset).toEqual({ theme: 'vault', scheme: 'dark' })
    expect(activeTheme({ mode: 'dark', light: 'ledger', dark: 'vault' })).toBe('vault')
    applyTheme(root, { mode: 'light', light: 'ledger', dark: 'vault' })
    expect(root.dataset).toEqual({ theme: 'ledger', scheme: 'light' })
    applyTheme(root, system)
    expect(root.dataset).toEqual({})
    expect(activeTheme(system)).toBe(null)
  })
})

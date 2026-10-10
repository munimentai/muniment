import { describe, expect, it } from 'vitest'

import {
  COLORS,
  PALETTES,
  STYLES,
  THEMES,
  THEME_DARK,
  THEME_LIGHT,
  THEME_STORAGE_KEY,
  THEME_SYSTEM,
  activeTheme,
  applyTheme,
  paletteName,
  parseTheme,
  serializeTheme,
  themeScheme,
} from './theme-state.js'

const system = { mode: THEME_SYSTEM, palette: 'sage' }

describe('theme state', () => {
  it('names four colors in three styles, each with a light and a dark theme', () => {
    expect(COLORS.map(([, name]) => name)).toEqual(['Sage', 'Harbor', 'Clay', 'Iris'])
    expect(STYLES.map(([, name]) => name)).toEqual(['Normal', 'Vibrant', 'High contrast'])
    expect(PALETTES).toEqual(['sage', 'harbor', 'clay', 'iris', 'sage-vibrant', 'harbor-vibrant', 'clay-vibrant', 'iris-vibrant', 'sage-contrast', 'harbor-contrast', 'clay-contrast', 'iris-contrast'])
    expect(THEMES).toHaveLength(24)
    expect(themeScheme('clay-vibrant-light')).toBe(THEME_LIGHT)
    expect(themeScheme('iris-contrast-dark')).toBe(THEME_DARK)
    expect(themeScheme('system')).toBe(null)
    expect(themeScheme('vault')).toBe(null)
    expect(paletteName('harbor-contrast')).toBe('Harbor high contrast')
    expect(paletteName('clay')).toBe('Clay')
  })

  it('round-trips each state and defaults missing or malformed data to system', () => {
    expect(THEME_STORAGE_KEY).toBe('muniment.theme')
    for (const state of [system, { mode: 'light', palette: 'harbor-vibrant' }, { mode: 'dark', palette: 'iris-contrast' }]) {
      expect(parseTheme(serializeTheme(state))).toEqual(state)
    }
    expect(parseTheme(null)).toEqual(system)
    expect(parseTheme('')).toEqual(system)
    expect(parseTheme('Dark')).toEqual(system)
    expect(parseTheme('{"theme":"dark"}')).toEqual(system)
    expect(parseTheme('{"mode":"dark","palette":"neon"}')).toEqual({ ...system, mode: 'dark' })
    expect(parseTheme('[1]')).toEqual(system)
    expect(serializeTheme('invalid')).toBe(JSON.stringify(system))
  })

  it('reads bare modes, bare theme names and the earlier light and dark picks', () => {
    expect(parseTheme('light')).toEqual({ ...system, mode: 'light' })
    expect(parseTheme('clay-vibrant-dark')).toEqual({ mode: 'dark', palette: 'clay-vibrant' })
    expect(parseTheme('moss')).toEqual({ mode: 'dark', palette: 'sage' })
    expect(parseTheme('vellum')).toEqual({ mode: 'light', palette: 'clay' })
    expect(parseTheme('carbon')).toEqual({ mode: 'dark', palette: 'sage-contrast' })
    expect(parseTheme('{"mode":"dark","light":"ledger","dark":"plum"}')).toEqual({ mode: 'dark', palette: 'iris' })
    expect(parseTheme('{"mode":"system","light":"daybreak","dark":"vault"}')).toEqual({ mode: 'system', palette: 'harbor-vibrant' })
  })

  it('applies the theme for the mode, follows the OS in System, and leaves the default palette to CSS', () => {
    const root = { dataset: {} }
    applyTheme(root, { mode: 'dark', palette: 'harbor' }, false)
    expect(root.dataset).toEqual({ theme: 'harbor-dark', scheme: 'dark' })
    applyTheme(root, { mode: 'system', palette: 'harbor' }, true)
    expect(root.dataset).toEqual({ theme: 'harbor-dark' })
    applyTheme(root, { mode: 'system', palette: 'harbor' }, false)
    expect(root.dataset).toEqual({ theme: 'harbor-light' })
    applyTheme(root, system, true)
    expect(root.dataset).toEqual({})
    expect(activeTheme(system, true)).toBe(null)
    expect(activeTheme({ mode: 'light', palette: 'sage' })).toBe('sage-light')
  })
})

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

const system = { mode: THEME_SYSTEM, palette: 'moss-vibrant' }
// Missing or malformed data restores the default: Moss vibrant dark.
const fallback = { mode: THEME_DARK, palette: 'moss-vibrant' }
// A saved value that names no palette keeps Moss.
const saved = { mode: THEME_DARK, palette: 'moss' }

describe('theme state', () => {
  it('names four colors in three styles, each with a light and a dark theme', () => {
    expect(COLORS.map(([, name]) => name)).toEqual(['Moss', 'Harbor', 'Clay', 'Iris'])
    expect(STYLES.map(([, name]) => name)).toEqual(['Normal', 'Vibrant', 'High contrast'])
    expect(PALETTES).toEqual(['moss', 'harbor', 'clay', 'iris', 'moss-vibrant', 'harbor-vibrant', 'clay-vibrant', 'iris-vibrant', 'moss-contrast', 'harbor-contrast', 'clay-contrast', 'iris-contrast'])
    expect(THEMES).toHaveLength(24)
    expect(themeScheme('clay-vibrant-light')).toBe(THEME_LIGHT)
    expect(themeScheme('iris-contrast-dark')).toBe(THEME_DARK)
    expect(themeScheme('system')).toBe(null)
    expect(themeScheme('vault')).toBe(null)
    expect(paletteName('harbor-contrast')).toBe('Harbor high contrast')
    expect(paletteName('clay')).toBe('Clay')
  })

  it('round-trips each state and defaults missing or malformed data to Moss vibrant dark', () => {
    expect(THEME_STORAGE_KEY).toBe('muniment.theme')
    for (const state of [system, fallback, { mode: 'light', palette: 'harbor-vibrant' }, { mode: 'dark', palette: 'iris-contrast' }]) {
      expect(parseTheme(serializeTheme(state))).toEqual(state)
    }
    expect(parseTheme(null)).toEqual(fallback)
    expect(parseTheme('')).toEqual(fallback)
    expect(parseTheme('Dark')).toEqual(fallback)
    expect(parseTheme('{"theme":"dark"}')).toEqual(saved)
    expect(parseTheme('{"mode":"light","palette":"neon"}')).toEqual({ ...saved, mode: 'light' })
    expect(parseTheme('[1]')).toEqual(fallback)
    expect(serializeTheme('invalid')).toBe(JSON.stringify(fallback))
  })

  it('reads bare modes, bare theme names and the earlier light and dark picks', () => {
    expect(parseTheme('light')).toEqual({ ...saved, mode: 'light' })
    expect(parseTheme('clay-vibrant-dark')).toEqual({ mode: 'dark', palette: 'clay-vibrant' })
    expect(parseTheme('moss')).toEqual({ mode: 'dark', palette: 'moss' })
    expect(parseTheme('vellum')).toEqual({ mode: 'light', palette: 'clay' })
    expect(parseTheme('carbon')).toEqual({ mode: 'dark', palette: 'moss-contrast' })
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
    expect(activeTheme({ mode: 'light', palette: 'moss' })).toBe('moss-light')
  })
})

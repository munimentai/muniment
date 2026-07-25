import { describe, expect, it } from 'vitest'

import {
  THEME_DARK,
  THEME_LIGHT,
  THEME_STORAGE_KEY,
  THEME_SYSTEM,
  parseTheme,
  serializeTheme,
} from './theme-state.js'

describe('theme state', () => {
  it('round-trips each choice and defaults missing or malformed data to system', () => {
    expect(THEME_STORAGE_KEY).toBe('muniment.theme')
    expect(parseTheme(serializeTheme(THEME_SYSTEM))).toBe(THEME_SYSTEM)
    expect(parseTheme(serializeTheme(THEME_LIGHT))).toBe(THEME_LIGHT)
    expect(parseTheme(serializeTheme(THEME_DARK))).toBe(THEME_DARK)
    expect(parseTheme(null)).toBe(THEME_SYSTEM)
    expect(parseTheme(undefined)).toBe(THEME_SYSTEM)
    expect(parseTheme('')).toBe(THEME_SYSTEM)
    expect(parseTheme('Dark')).toBe(THEME_SYSTEM)
    expect(parseTheme('{"theme":"dark"}')).toBe(THEME_SYSTEM)
    expect(serializeTheme('invalid')).toBe(THEME_SYSTEM)
  })
})

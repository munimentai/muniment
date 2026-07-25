export const THEME_STORAGE_KEY = 'muniment.theme'
export const THEME_SYSTEM = 'system'
export const THEME_LIGHT = 'light'
export const THEME_DARK = 'dark'

const themes = new Set([THEME_SYSTEM, THEME_LIGHT, THEME_DARK])

// Missing, malformed, or older values restore the documented OS default.
export function parseTheme(stored) {
  return themes.has(stored) ? stored : THEME_SYSTEM
}

export function serializeTheme(theme) {
  return parseTheme(theme)
}

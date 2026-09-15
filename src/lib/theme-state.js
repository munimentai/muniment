export const THEME_STORAGE_KEY = 'muniment.theme'
export const THEME_SYSTEM = 'system'
export const THEME_LIGHT = 'light'
export const THEME_DARK = 'dark'

// Eight themes: four light, four dark. Each is one `:root[data-theme]` block of
// the ten color tokens in src/styles/tokens.css. System follows the OS and
// uses the default of each scheme.
export const LIGHT_THEMES = ['paper', 'vellum', 'ledger', 'foolscap']
export const DARK_THEMES = ['moss', 'vault', 'graphite', 'inkwell']
export const DEFAULT_LIGHT_THEME = 'paper'
export const DEFAULT_DARK_THEME = 'vault'
export const THEME_NAMES = {
  paper: 'Paper', vellum: 'Vellum', ledger: 'Ledger', foolscap: 'Foolscap',
  moss: 'Moss', vault: 'Vault', graphite: 'Graphite', inkwell: 'Inkwell',
}

const modes = new Set([THEME_SYSTEM, THEME_LIGHT, THEME_DARK])

export function themeScheme(theme) {
  if (LIGHT_THEMES.includes(theme)) return THEME_LIGHT
  if (DARK_THEMES.includes(theme)) return THEME_DARK
  return null
}

const defaults = () => ({ mode: THEME_SYSTEM, light: DEFAULT_LIGHT_THEME, dark: DEFAULT_DARK_THEME })

// Missing, malformed, or older values restore the documented OS default. An
// older bare mode keeps that mode with its default theme, and a bare theme
// name picks that theme in its own mode.
export function parseTheme(stored) {
  const state = defaults()
  if (stored && typeof stored === 'object') return parseTheme(JSON.stringify(stored))
  if (typeof stored !== 'string' || !stored) return state
  if (modes.has(stored)) return { ...state, mode: stored }
  const scheme = themeScheme(stored)
  if (scheme) return { ...state, mode: scheme, [scheme]: stored }
  let parsed
  try { parsed = JSON.parse(stored) } catch (_) { return state }
  if (!parsed || typeof parsed !== 'object') return state
  if (modes.has(parsed.mode)) state.mode = parsed.mode
  if (LIGHT_THEMES.includes(parsed.light)) state.light = parsed.light
  if (DARK_THEMES.includes(parsed.dark)) state.dark = parsed.dark
  return state
}

export function serializeTheme(theme) {
  const { mode, light, dark } = parseTheme(theme)
  return JSON.stringify({ mode, light, dark })
}

// The theme a state shows in an explicit mode, or null in System.
export function activeTheme(theme) {
  return theme.mode === THEME_SYSTEM ? null : theme[theme.mode]
}

// The root carries the picked theme and its scheme. System carries nothing, so
// the OS media query decides between the two defaults.
export function applyTheme(root, theme) {
  const active = activeTheme(theme)
  if (active) {
    root.dataset.theme = active
    root.dataset.scheme = theme.mode
  } else {
    delete root.dataset.theme
    delete root.dataset.scheme
  }
}

export function readStoredTheme() {
  try {
    return parseTheme(localStorage.getItem(THEME_STORAGE_KEY))
  } catch (_) {
    return parseTheme(null)
  }
}

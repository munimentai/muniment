export const THEME_STORAGE_KEY = 'muniment.theme'
export const THEME_SYSTEM = 'system'
export const THEME_LIGHT = 'light'
export const THEME_DARK = 'dark'

// The color schemes: four colors, each in three styles, and each style with a
// light and a dark version. A palette is one color in one style. Mode picks
// its light or dark version, and System follows the OS. Each version is one
// `:root[data-theme]` block of the ten color tokens in src/styles/tokens.css.
export const COLORS = [['sage', 'Sage'], ['harbor', 'Harbor'], ['clay', 'Clay'], ['iris', 'Iris']]
export const STYLES = [['normal', 'Normal'], ['vibrant', 'Vibrant'], ['contrast', 'High contrast']]
export const DEFAULT_PALETTE = 'sage'

export const paletteId = (color, style) => style === 'normal' ? color : `${color}-${style}`
export const PALETTES = STYLES.flatMap(([style]) => COLORS.map(([color]) => paletteId(color, style)))
export const themeId = (palette, scheme) => `${palette}-${scheme}`
export const THEMES = PALETTES.flatMap((palette) => [themeId(palette, THEME_LIGHT), themeId(palette, THEME_DARK)])

// A palette's accessible name: the color, then its style when it has one.
export function paletteName(palette) {
  const [color, style = 'normal'] = palette.split('-')
  const colorName = COLORS.find(([id]) => id === color)?.[1] ?? color
  return style === 'normal' ? colorName : `${colorName} ${STYLES.find(([id]) => id === style)[1].toLowerCase()}`
}

export function themeScheme(theme) {
  if (!THEMES.includes(theme)) return null
  return theme.endsWith(`-${THEME_DARK}`) ? THEME_DARK : THEME_LIGHT
}

// A stored value from the earlier theme list names one theme. Each maps to
// the palette nearest it, and the dark ones keep the dark mode.
const EARLIER = {
  paper: 'sage', foolscap: 'sage', vault: 'sage', moss: 'sage', graphite: 'sage',
  ledger: 'harbor', inkwell: 'harbor', fjord: 'harbor', nocturne: 'harbor', lagoon: 'harbor', obsidian: 'harbor',
  vellum: 'clay', parchment: 'clay', manila: 'clay', umber: 'clay', basalt: 'clay',
  linen: 'iris', plum: 'iris', nightshade: 'iris',
  broadsheet: 'sage-contrast', carbon: 'sage-contrast',
  daybreak: 'harbor-vibrant', afterglow: 'harbor-vibrant', canopy: 'sage-vibrant', coral: 'clay-vibrant',
}
const EARLIER_DARK = new Set(['vault', 'moss', 'graphite', 'inkwell', 'fjord', 'nocturne', 'lagoon', 'obsidian', 'umber', 'basalt', 'plum', 'nightshade', 'carbon', 'afterglow', 'canopy'])

const modes = new Set([THEME_SYSTEM, THEME_LIGHT, THEME_DARK])
const defaults = () => ({ mode: THEME_SYSTEM, palette: DEFAULT_PALETTE })

// Missing or malformed values restore the documented OS default. A bare mode
// keeps that mode with the default palette, and a bare theme name picks its
// palette in its own mode.
export function parseTheme(stored) {
  const state = defaults()
  if (stored && typeof stored === 'object') return parseTheme(JSON.stringify(stored))
  if (typeof stored !== 'string' || !stored) return state
  if (modes.has(stored)) return { ...state, mode: stored }
  const scheme = themeScheme(stored)
  if (scheme) return { mode: scheme, palette: stored.slice(0, -scheme.length - 1) }
  if (EARLIER[stored]) return { mode: EARLIER_DARK.has(stored) ? THEME_DARK : THEME_LIGHT, palette: EARLIER[stored] }
  let parsed
  try { parsed = JSON.parse(stored) } catch (_) { return state }
  if (!parsed || typeof parsed !== 'object') return state
  if (modes.has(parsed.mode)) state.mode = parsed.mode
  if (PALETTES.includes(parsed.palette)) state.palette = parsed.palette
  else {
    const earlier = parsed[state.mode === THEME_DARK ? 'dark' : 'light']
    if (EARLIER[earlier]) state.palette = EARLIER[earlier]
  }
  return state
}

export function serializeTheme(theme) {
  const { mode, palette } = parseTheme(theme)
  return JSON.stringify({ mode, palette })
}

export function systemPrefersDark() {
  try { return typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches } catch (_) { return false }
}

// The theme a state shows. System with the default palette shows nothing, so
// the OS media query decides between the two defaults.
export function activeTheme(theme, prefersDark = systemPrefersDark()) {
  if (theme.mode !== THEME_SYSTEM) return themeId(theme.palette, theme.mode)
  if (theme.palette === DEFAULT_PALETTE) return null
  return themeId(theme.palette, prefersDark ? THEME_DARK : THEME_LIGHT)
}

// The root carries the theme, and its scheme when the mode names one.
export function applyTheme(root, theme, prefersDark = systemPrefersDark()) {
  const active = activeTheme(theme, prefersDark)
  if (active) root.dataset.theme = active
  else delete root.dataset.theme
  if (theme.mode === THEME_SYSTEM) delete root.dataset.scheme
  else root.dataset.scheme = theme.mode
}

export function readStoredTheme() {
  try {
    return parseTheme(localStorage.getItem(THEME_STORAGE_KEY))
  } catch (_) {
    return parseTheme(null)
  }
}

// What every agent avatar on screen draws with: the picked avatar style and
// the light or dark tone of the theme in use. One shared state follows the
// style pick, the theme on the root and, when the app follows the system, the
// OS appearance, so each avatar redraws when any of them changes.
import { AVATAR_STYLE_EVENT, AVATAR_STYLE_STORAGE_KEY, parseAvatarStyle, readStoredAvatarStyle } from './avatar-style-state.js'
import { systemPrefersDark, themeScheme } from './theme-state.js'

// The tone the root shows: the scheme of its theme, or the OS appearance when
// the root names none and the tokens follow the media query.
export function documentTone(root = globalThis.document?.documentElement) {
  const scheme = themeScheme(root?.dataset?.theme)
  if (scheme) return scheme
  if (root?.dataset?.scheme === 'light' || root?.dataset?.scheme === 'dark') return root.dataset.scheme
  return systemPrefersDark() ? 'dark' : 'light'
}

const display = $state({ style: readStoredAvatarStyle(), tone: 'dark' })
let watching = false

function watch() {
  if (watching || typeof window === 'undefined') return
  watching = true
  const root = document.documentElement
  display.tone = documentTone(root)
  const follow = () => { display.tone = documentTone(root) }
  window.addEventListener(AVATAR_STYLE_EVENT, (event) => { display.style = parseAvatarStyle(event.detail) })
  window.addEventListener('storage', (event) => { if (event.key === AVATAR_STYLE_STORAGE_KEY) display.style = parseAvatarStyle(event.newValue) })
  try { new MutationObserver(follow).observe(root, { attributes: true, attributeFilter: ['data-theme', 'data-scheme'] }) } catch (_) {}
  try { matchMedia('(prefers-color-scheme: dark)').addEventListener('change', follow) } catch (_) {}
}

// The shared state, `{ style, tone }`. Reading it inside a component makes
// that component redraw when it changes.
export function avatarDisplay() {
  watch()
  return display
}

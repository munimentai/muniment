// The main window opens hidden on the default theme's background. Once the
// page has drawn in the stored theme, the shell takes the page's background
// for the window and the webview and shows the window, so launch shows no
// white frame. A theme change sends the new background, so a resize never
// shows the old one.
import { THEME_EVENT } from './theme-state.js'

export function pageBackground(doc = document) {
  try { return getComputedStyle(doc.body).backgroundColor || null } catch (_) { return null }
}

export function reportReady(tauri, doc = document) {
  return Promise.resolve(tauri?.invoke('main_window_ready', { background: pageBackground(doc) })).catch(() => {})
}

// Reports the first frame, then each theme change. Answers the function that stops it.
export function revealMainWindow(tauri, { doc = document, view = window } = {}) {
  if (!tauri) return () => {}
  void reportReady(tauri, doc)
  const follow = () => { void reportReady(tauri, doc) }
  view.addEventListener(THEME_EVENT, follow)
  let scheme = null
  try { scheme = view.matchMedia('(prefers-color-scheme: dark)'); scheme.addEventListener('change', follow) } catch (_) { scheme = null }
  return () => {
    view.removeEventListener(THEME_EVENT, follow)
    scheme?.removeEventListener('change', follow)
  }
}

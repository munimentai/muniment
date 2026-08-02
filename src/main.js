import { mount } from 'svelte'
import './styles/tokens.css'
import './styles/base.css'
import './styles/code-diff.css'
import App from './App.svelte'
import { THEME_STORAGE_KEY, parseTheme } from './lib/theme-state.js'

let theme
try {
  theme = parseTheme(localStorage.getItem(THEME_STORAGE_KEY))
} catch (_) {
  theme = parseTheme(null)
}
if (theme === 'system') delete document.documentElement.dataset.theme
else document.documentElement.dataset.theme = theme
mount(App, { target: document.getElementById('app') })

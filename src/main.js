import { mount } from 'svelte'
import './styles/tokens.css'
import './styles/base.css'
import './styles/code-diff.css'
import App from './App.svelte'
import Launcher from './Launcher.svelte'
import { applyTheme, readStoredTheme } from './lib/theme-state.js'

applyTheme(document.documentElement, readStoredTheme())
mount(new URLSearchParams(location.search).has('launcher') ? Launcher : App, {
  target: document.getElementById('app'),
})

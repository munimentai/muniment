import { mount } from 'svelte'
import './styles/tokens.css'
import './styles/base.css'
import './styles/panels.css'
import './styles/scrollbars.css'
import './styles/selection.css'
import './styles/code-diff.css'
import App from './App.svelte'
import Launcher from './Launcher.svelte'
import WorkspacePopup from './lib/WorkspacePopup.svelte'
import { applyTheme, readStoredTheme } from './lib/theme-state.js'
import { applyType, readStoredType } from './lib/type-state.js'

applyTheme(document.documentElement, readStoredTheme())
applyType(document.documentElement, readStoredType())
const query = new URLSearchParams(location.search)
if (query.has('workspace-menu')) document.documentElement.classList.add('workspace-popup-window')
mount(query.has('workspace-menu') ? WorkspacePopup : query.has('launcher') ? Launcher : App, {
  target: document.getElementById('app'),
})

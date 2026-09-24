import { mount } from 'svelte'
import './styles/tokens.css'
import './styles/base.css'
import './styles/panels.css'
import './styles/scrollbars.css'
import './styles/selection.css'
import './styles/code-diff.css'
import { applyTheme, readStoredTheme } from './lib/theme-state.js'
import { applyType, readStoredType } from './lib/type-state.js'

applyTheme(document.documentElement, readStoredTheme())
applyType(document.documentElement, readStoredType())
const query = new URLSearchParams(location.search)
if (query.has('workspace-menu')) document.documentElement.classList.add('workspace-popup-window')
// Each window loads only its own root component, so the launcher and the
// workspace popup never load the main window's code.
const { default: Root } = await (query.has('workspace-menu')
  ? import('./lib/WorkspacePopup.svelte')
  : query.has('launcher') ? import('./Launcher.svelte') : import('./App.svelte'))
mount(Root, {
  target: document.getElementById('app'),
})

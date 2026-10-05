import { mount } from 'svelte'
import Demo from './Demo.svelte'
import '@desktop/styles/tokens.css'
import '@desktop/styles/panels.css'
mount(Demo, { target: document.getElementById('app') })

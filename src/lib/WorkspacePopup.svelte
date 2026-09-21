<script>
  import { onMount, tick } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { listen, emitTo } from '@tauri-apps/api/event'
  import { applyTheme, readStoredTheme } from './theme-state.js'
  import LucideIcon from './LucideIcon.svelte'
  let selected = $state(null)
  let actionId = $state(null)
  let label = $state('Workspace tools')
  const workspaceItems = [{id:'browser',name:'Browser',icon:'globe'},{id:'files',name:'Files',icon:'folder'},{id:'terminal',name:'Terminal',icon:'square-terminal'}]
  let items = $state(workspaceItems)
  let menu
  let shown = false
  const popup = getCurrentWindow()
  async function close() { if (!shown) return; shown = false; await popup.hide(); await emitTo('main', actionId ? 'action-menu-closed' : 'workspace-menu-closed', actionId) }
  async function choose(id) { const source = actionId; shown = false; await popup.hide(); await emitTo('main', source ? 'action-menu-select' : 'workspace-menu-select', source ? {id: source, item: id} : id) }
  function keys(event) {
    const buttons = [...menu.querySelectorAll('button:not(:disabled)')]
    const index = buttons.indexOf(document.activeElement)
    if (event.key === 'Escape') { event.preventDefault(); void close() }
    else if (['ArrowDown','ArrowUp','Home','End'].includes(event.key)) {
      event.preventDefault()
      buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length]?.focus()
    }
  }
  onMount(() => {
    let disposed = false
    const stops = []
    const retain = off => { if (disposed) off(); else stops.push(off) }
    void listen('workspace-menu-open', async ({payload}) => {
      actionId = null
      label = 'Workspace tools'
      items = workspaceItems
      selected = payload.selected
      applyTheme(document.documentElement, readStoredTheme())
      await tick()
      shown = true; await popup.show()
      await popup.setFocus()
      menu.querySelector('button')?.focus()
    }).then(retain)
    void listen('action-menu-open', async ({payload}) => {
      actionId = payload.id; label = payload.label; items = payload.items; selected = null
      applyTheme(document.documentElement, readStoredTheme())
      await tick(); shown = true; await popup.show(); await popup.setFocus(); menu.querySelector('button:not(:disabled)')?.focus()
    }).then(retain)
    void popup.onFocusChanged(({payload}) => { if (!payload && shown) void close() }).then(retain)
    return () => { disposed = true; stops.forEach(off => off()) }
  })
</script>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div data-panel="workspace-menu" data-panel-variant="overlay" class="tools" role="menu" tabindex="-1" aria-label={label} bind:this={menu} onkeydown={keys}>
  {#each items as item}
    <button type="button" role="menuitem" disabled={item.disabled} onclick={() => choose(item.id)}>{#if item.icon}<LucideIcon name={item.icon}/>{/if}<span>{item.name}</span>{#if !actionId && selected === item.id}<LucideIcon name="check"/>{/if}</button>
  {/each}
</div>
<style>
  :global(html.workspace-popup-window), :global(.workspace-popup-window body), :global(.workspace-popup-window #app) { background: transparent !important; margin: 0; overflow: hidden; }
  .tools { margin: 8px; padding: 6px;     }
  button { display:flex; align-items:center; gap:10px; width:100%; min-height:36px; padding:8px; border:0; text-align:left; border-radius:var(--radius-control); color:var(--ink); background:transparent; cursor:pointer; font:var(--text-13) var(--font-human); }
  button:disabled { opacity: .5; cursor: default; }
  button:hover:not(:disabled), button:focus-visible { background:var(--faint); } span { flex:1; }
</style>

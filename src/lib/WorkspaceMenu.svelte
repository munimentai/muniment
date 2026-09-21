<script>
  import { onMount, onDestroy, tick } from 'svelte'
  import { Window, getCurrentWindow } from '@tauri-apps/api/window'
  import { listen } from '@tauri-apps/api/event'
  import { PhysicalPosition, LogicalSize } from '@tauri-apps/api/dpi'
  import LucideIcon from './LucideIcon.svelte'
  let { selected = null, onselect, onopenchange, label = 'Workspace tools', icon = 'ellipsis' } = $props()
  let expanded = $state(false)
  let element
  let trigger
  let popupWindow
  let opening = false
  const items = [{id:'browser',name:'Browser',icon:'globe'},{id:'files',name:'Files',icon:'folder'},{id:'terminal',name:'Terminal',icon:'square-terminal'}]
  async function toggle() {
    if (opening) return
    if (!expanded && window.__TAURI_INTERNALS__) {
      opening = true
      try {
        popupWindow = await Window.getByLabel('workspace-menu')
        if (!popupWindow) throw new Error('Workspace popup unavailable')
        const main = getCurrentWindow()
        const [origin, scale] = await Promise.all([main.innerPosition(), main.scaleFactor()])
        const rect = trigger.getBoundingClientRect()
        await popupWindow.setSize(new LogicalSize(240, 140))
        await popupWindow.setPosition(new PhysicalPosition(Math.round(origin.x + Math.max(0, rect.right - 232) * scale), Math.round(origin.y + (rect.bottom - 2) * scale)))
        await popupWindow.emit('workspace-menu-open', {selected})
        return
      } catch { /* Browser previews use the HTML menu. */ }
      finally { opening = false }
    }
    expanded = !expanded
    onopenchange?.(expanded)
    if (expanded) { await tick(); element.querySelector('[role="menuitem"]')?.focus() }
  }
  function close(focus = false) { expanded = false; onopenchange?.(false); if (focus) trigger?.focus() }
  function keys(event) {
    const buttons = [...element.querySelectorAll('[role="menuitem"]')]
    const i = buttons.indexOf(document.activeElement)
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(true) }
    else if (['ArrowDown','ArrowUp','Home','End'].includes(event.key)) {
      event.preventDefault()
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length-1 : (i+(event.key==='ArrowDown'?1:-1)+buttons.length)%buttons.length
      buttons[next]?.focus()
    }
  }
  onDestroy(() => { void popupWindow?.hide().catch(() => {}) })
  onMount(() => {
    if (!window.__TAURI_INTERNALS__) return
    let disposed = false
    const stops = []
    const retain = off => { if (disposed) off(); else stops.push(off) }
    void listen('workspace-menu-select', ({payload}) => { if (items.some(item => item.id === payload)) onselect(selected === payload ? null : payload) }).then(retain).catch(() => {})
    void getCurrentWindow().onMoved(() => { void popupWindow?.hide() }).then(retain).catch(() => {})
    void getCurrentWindow().onResized(() => { void popupWindow?.hide() }).then(retain).catch(() => {})
    return () => { disposed = true; stops.forEach(off => off()) }
  })
  onMount(() => { const outside = e => { if (!element.contains(e.target)) close() }; document.addEventListener('pointerdown',outside); return () => document.removeEventListener('pointerdown',outside) })
</script>
<div class="workspace-menu" bind:this={element}>
  <button class="trigger" type="button" aria-label={label} aria-haspopup="menu" aria-expanded={expanded} bind:this={trigger} onclick={toggle}><LucideIcon name={icon} variant="action" /></button>
  {#if expanded}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div data-panel="workspace-menu" data-panel-variant="overlay" class="tools" role="menu" tabindex="-1" aria-label={label} onkeydown={keys}>
      {#each items as item}
        <button type="button" role="menuitem" onclick={() => { close(true); onselect(selected === item.id ? null : item.id) }}><LucideIcon name={item.icon} /><span>{item.name}</span>{#if selected === item.id}<LucideIcon name="check" />{/if}</button>
      {/each}
    </div>
  {/if}
</div>
<style>
  :global(.titlebar-thread) .workspace-menu { order:4; }
  .workspace-menu { position:relative; flex:none; }
  button { display:flex; align-items:center; gap:10px; border:0; color:var(--ink); background:transparent; cursor:pointer; font:var(--text-13) var(--font-human); }
  .trigger { width:28px; height:28px; justify-content:center; padding:4px; border-radius:var(--radius-control); }
  button:hover { background:var(--faint); }
  .tools { position:absolute; right:0; top:calc(100% + 6px); z-index:120; width:220px; padding:6px;     }
  .tools button { width:100%; min-height:36px; padding:8px; text-align:left; border-radius:var(--radius-control); } span { flex:1; }
</style>

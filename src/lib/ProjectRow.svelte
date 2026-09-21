<script>
  import { floatingMenu } from './floating-menu.js'
  import { tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { name, expanded, disabled = false, ontoggle, onnew, onrename, onopen, icon = 'folder', label = `Project ${name}`, newLabel = `New thread in ${name}`, onactivate = ontoggle, panelOpen = undefined } = $props()
  let open = $state(false)
  let row = $state()
  let trigger = $state()
  let menu = $state()
  $effect(() => {
    if (!open) return
    const outside = event => { if (!row?.contains(event.target)) open = false }
    document.addEventListener('pointerdown', outside, true)
    return () => document.removeEventListener('pointerdown', outside, true)
  })
  async function toggleMenu() {
    open = !open
    if (open) { await tick(); menu?.querySelector('button:not(:disabled)')?.focus() }
  }
  function contextMenu(event) { if (!onrename || !onopen) return; event.preventDefault(); void toggleMenu() }
  function contextKey(event) { if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) contextMenu(event) }
  function choose(action) { open = false; trigger?.focus(); action() }
  function menuKey(event) {
    if (event.key === 'Escape') { event.preventDefault(); open = false; trigger?.focus(); return }
    const items = [...menu.querySelectorAll('button:not(:disabled)')]
    const index = items.indexOf(document.activeElement)
    const next = event.key === 'ArrowDown' ? (index + 1) % items.length : event.key === 'ArrowUp' ? (index + items.length - 1) % items.length : event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : -1
    if (next >= 0) { event.preventDefault(); items[next].focus() }
    if (event.key === 'Tab') open = false
  }
</script>

<div class="project-row" bind:this={row}>
  <button class="project-title" aria-label={label} aria-expanded={panelOpen ?? (icon === 'folder' ? expanded : undefined)} onclick={onactivate} oncontextmenu={contextMenu} onkeydown={contextKey}><LucideIcon name={icon === 'folder' && expanded ? 'folder-open' : icon} /><span>{name}</span></button>
  <button class="project-control" aria-label={newLabel} {disabled} onclick={onnew}><LucideIcon name="square-pen" size={14} /></button>
  <button class="project-control" class:visible={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} ${name}`} aria-expanded={expanded} bind:this={trigger} onclick={ontoggle}><LucideIcon name={expanded ? 'chevron-down' : 'chevron-right'} size={14} /></button>
  {#if open}
    <div class="project-menu" use:floatingMenu role="menu" tabindex="-1" aria-label={`${name} actions`} bind:this={menu} onkeydown={menuKey}>
      <button role="menuitem" {disabled} onclick={() => choose(onnew)}><LucideIcon name="square-pen" size={14} />New thread</button>
      <button role="menuitem" onclick={() => choose(onrename)}><LucideIcon name="pencil" size={14} />Rename</button>
      <button role="menuitem" onclick={() => choose(onopen)}><LucideIcon name="folder-open" size={14} />Open folder</button>
    </div>
  {/if}
</div>

<style>
  .project-row { display: flex; align-items: center; position: relative; }
  .project-title { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; padding: 6px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; }
  .project-title span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .project-title :global(.lucide) { flex: none; width: 18px; height: 18px; }
  .project-title:hover { background: var(--faint); }
  .project-control { flex: none; display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; padding: 4px; border-radius: var(--radius-control); opacity: 0; border: 0; background: transparent; color: var(--muted); }
  .project-row:hover .project-control, .project-row:focus-within .project-control, .project-control.visible { opacity: 1; }
  .project-control:hover:not(:disabled), .project-control:focus-visible { background: var(--faint); color: var(--ink); }
  @media (hover: none) { .project-control { opacity: 1; } }
  .project-menu { position: absolute; right: 0; top: 100%; z-index: 20; min-width: 140px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); }
  .project-menu button { display: flex; align-items: center; gap: 8px; width: 100%; padding: 5px 8px; border: 0; background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; }
  .project-menu button:disabled { opacity: .55; cursor: default; }
  .project-menu button:hover:not(:disabled), .project-menu button:focus-visible { background: var(--faint); }
</style>

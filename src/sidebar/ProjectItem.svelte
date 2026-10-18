<script>
  // One row of the sidebar's Projects list: the project's name with its
  // triggers in words, such as `every day 9:00` or `webhook`, the pencil that
  // starts a thread in it, and its chevron. The menu holds Settings, Rename,
  // Open folder and New group chat where the project has each.
  import { floatingMenu } from '../lib/floating-menu.js'
  import { tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import OwnerChip from '../runtimes/OwnerChip.svelte'
  // `owner` names the fleet member that holds a shared project's files.
  let { owner = null, name, triggers = '', kind = 'project', expanded, disabled = false, ontoggle, onnew, onsettings, onrename, onopen, ongroup } = $props()
  let open = $state(false)
  let row = $state()
  let trigger = $state()
  let menu = $state()
  const icon = $derived(kind === 'chats' ? 'message-square' : kind === 'agent' ? 'bot' : expanded ? 'folder-open' : 'folder')
  const actions = $derived([
    onsettings && { id: 'settings', label: 'Settings', icon: 'settings', run: onsettings },
    onrename && { id: 'rename', label: 'Rename', icon: 'pencil', run: onrename },
    onopen && { id: 'open', label: 'Open folder', icon: 'folder-open', run: onopen },
    ongroup && { id: 'group', label: 'New group chat', icon: 'users', run: ongroup },
  ].filter(Boolean))
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
  function contextMenu(event) { if (!actions.length) return; event.preventDefault(); void toggleMenu() }
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
  <button class="project-title" aria-label={triggers ? `Project ${name}, ${triggers}` : `Project ${name}`} aria-expanded={expanded} onclick={ontoggle} oncontextmenu={contextMenu} onkeydown={contextKey}><LucideIcon name={icon} /><span>{name}</span>{#if triggers}<em class="triggers">{triggers}</em>{/if}{#if owner}<OwnerChip machine={owner} prefix={kind === 'agent' ? 'on' : 'files on'} compact />{/if}</button>
  <!-- The controls sit over the row's right end and show on hover or focus, so the triggers keep the row's width at rest. -->
  <span class="project-controls" class:visible={open}>
    <button class="project-control" aria-label={`New thread in ${name}`} {disabled} onclick={onnew}><LucideIcon name="square-pen" size={14} /></button>
    {#if actions.length}<button class="project-control" aria-label={`${name} actions`} aria-haspopup="menu" aria-expanded={open} bind:this={trigger} onclick={toggleMenu}><LucideIcon name="ellipsis-vertical" size={14} /></button>{/if}
  </span>
  {#if open}
    <div class="project-menu" use:floatingMenu role="menu" tabindex="-1" aria-label={`${name} actions`} bind:this={menu} onkeydown={menuKey}>
      <button role="menuitem" {disabled} onclick={() => choose(onnew)}><LucideIcon name="square-pen" size={14} />New thread</button>
      {#each actions as action (action.id)}<button role="menuitem" onclick={() => choose(action.run)}><LucideIcon name={action.icon} size={14} />{action.label}</button>{/each}
    </div>
  {/if}
</div>

<style>
  .project-row { display: flex; align-items: center; position: relative; border-radius: var(--radius-control); }
  .project-title { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; min-height: 28px; padding: 4px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; }
  .project-title span { flex: 0 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; }
  .project-title :global(span[data-overflow="true"]) { mask-image: linear-gradient(to right, currentColor calc(100% - 28px), transparent); }
  .project-title :global(.lucide) { flex: none; width: 16px; height: 16px; color: var(--muted); }
  .triggers { flex: 0 3 auto; min-width: 24px; margin-left: auto; overflow: hidden; color: var(--muted); font: var(--text-12) var(--font-human); font-style: normal; white-space: nowrap; text-overflow: ellipsis; }
  .project-title:hover { background: var(--faint); }
  .project-controls { position: absolute; top: 0; right: 0; bottom: 0; display: flex; align-items: center; border-radius: var(--radius-control); background: var(--faint); opacity: 0; pointer-events: none; }
  .project-row:hover .project-controls, .project-row:focus-within .project-controls, .project-controls.visible { opacity: 1; pointer-events: auto; }
  .project-control { flex: none; display: flex; align-items: center; justify-content: center; width: 24px; height: 28px; padding: 4px; border-radius: var(--radius-control); border: 0; background: transparent; color: var(--muted); }
  .project-control:hover:not(:disabled), .project-control:focus-visible { background: color-mix(in srgb, var(--ink) 8%, var(--faint)); color: var(--ink); }
  @media (hover: none) { .project-controls { position: static; opacity: 1; pointer-events: auto; background: transparent; } }
  .project-menu { position: absolute; right: 0; top: 100%; z-index: 20; min-width: 160px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); }
  .project-menu button { display: flex; align-items: center; gap: 8px; width: 100%; padding: 5px 8px; border: 0; background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; }
  .project-menu button:disabled { opacity: .55; cursor: default; }
  .project-menu button:hover:not(:disabled), .project-menu button:focus-visible { background: var(--faint); }
</style>

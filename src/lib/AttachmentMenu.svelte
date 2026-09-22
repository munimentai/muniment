<script>
  import { onMount, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from './composer-panels.js'
  let { onfiles, onfolder } = $props()
  let open = $state(false), root, trigger
  function close() { openComposerPanel(null) }
  async function toggle() { openComposerPanel(open ? null : 'attachments'); await tick(); root?.querySelector('[role=menuitem]')?.focus() }
  function keys(event) {
    if (!open) return
    if (event.key === 'Escape') { event.stopPropagation(); close(); trigger?.focus() }
    if (['ArrowDown', 'ArrowUp'].includes(event.key)) {
      event.preventDefault()
      const items = [...root.querySelectorAll('[role=menuitem]')]
      items[(items.indexOf(document.activeElement) + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus()
    }
  }
  onMount(() => {
    const follow = event => { open = event.detail === 'attachments' }
    window.addEventListener(COMPOSER_PANEL_EVENT, follow)
    return () => window.removeEventListener(COMPOSER_PANEL_EVENT, follow)
  })
</script>
<svelte:window onkeydown={keys} onpointerdown={event => { if (open && !root?.contains(event.target)) close() }} />
<div bind:this={root}>
  <button class="trigger" type="button" bind:this={trigger} aria-label="Add files or folders" aria-haspopup="menu" aria-expanded={open} onclick={toggle}><LucideIcon name="paperclip" size={16} /></button>
  {#if open}<div class="menu" data-composer-panel data-panel="attachments" data-panel-variant="overlay" role="menu" aria-label="Attach">
    <button type="button" role="menuitem" onclick={() => { close(); onfiles() }}><LucideIcon name="file-text" />Add files</button>
    <button type="button" role="menuitem" onclick={() => { close(); onfolder() }}><LucideIcon name="folder" />Add folder</button>
  </div>{/if}
</div>
<style>
  button { display: flex; align-items: center; gap: 8px; background: transparent; border: 0; color: var(--ink); font: inherit; cursor: pointer; border-radius: var(--radius-control); padding: 7px 9px; }
  button:hover, button:focus-visible { background: var(--faint); }
  .trigger { padding: 4px; min-width: 24px; min-height: 24px; }
  .menu { padding: 6px; width: 180px; }
  .menu button { width: 100%; }
</style>

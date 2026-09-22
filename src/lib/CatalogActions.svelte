<script>
  import PopupClose from './PopupClose.svelte'
  import { floatingMenu } from './floating-menu.js'
  import { tick, onMount } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { name, archived = false, onaction, disabled = false, extraActions = [], allowArchive = true, allowDelete = true, maxNameLength = 160 } = $props()
  let open = $state(false), mode = $state('menu'), value = $state(''), error = $state(''), busy = $state(false)
  let trigger, menu
  onMount(() => {
    document.addEventListener('keydown', keys, true)
    return () => document.removeEventListener('keydown', keys, true)
  })
  function portal(node) {
    document.body.appendChild(node)
    const rect = trigger.getBoundingClientRect()
    node.style.left = `${Math.max(8, Math.min(rect.right - 220, innerWidth - 228))}px`
    node.style.top = `${Math.max(8, Math.min(rect.bottom + 4, innerHeight - 200))}px`
    node.querySelector('button')?.focus()
    return { destroy() { node.remove() } }
  }
  function close() { if (!busy) { open = false; trigger?.focus() } }
  async function choose(action) {
    busy = true; error = ''
    try { await onaction(action, value.trim()); open = false; trigger?.focus() }
    catch (e) { error = String(e) }
    finally { busy = false }
  }
  function outside(event) { if (open && !menu?.contains(event.target) && !trigger?.contains(event.target)) close() }
  function keys(event) {
    if (!open) return
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close() }
    if (mode === 'menu' && ['ArrowDown','ArrowUp','Home','End'].includes(event.key)) {
      event.preventDefault()
      const items = [...menu.querySelectorAll('button')], index = items.indexOf(document.activeElement)
      items[event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus()
    }
  }
</script>
<svelte:window onpointerdown={outside}/>
<button class="catalog-actions" class:opened={open} bind:this={trigger} aria-label={`Actions for ${name}`} aria-haspopup="menu" aria-expanded={open} {disabled} onclick={() => { mode = 'menu'; error = ''; open = !open }}><LucideIcon name="ellipsis-vertical" size={16}/></button>
{#if open}
  <div class="catalog-menu" data-panel="catalog-menu" data-panel-variant="overlay" bind:this={menu} use:portal use:floatingMenu role={mode === 'menu' ? 'menu' : 'dialog'} aria-label={`Actions for ${name}`}>
    {#if mode === 'menu'}
      {#each extraActions as action}<button role="menuitem" disabled={busy} onclick={() => choose(action.id)}><LucideIcon name={action.icon} size={16}/>{action.label}</button>{/each}
      <button role="menuitem" onclick={async () => { value = name; mode = 'rename'; await tick(); menu.querySelector('input')?.select() }}><LucideIcon name="pencil" size={16}/>Rename</button>
      {#if allowArchive}<button role="menuitem" onclick={() => choose(archived ? 'restore' : 'archive')} disabled={busy}><LucideIcon name="archive" size={16}/>{archived ? 'Restore' : 'Archive'}</button>{/if}
      {#if allowDelete}<button role="menuitem" onclick={() => mode = 'delete'}><LucideIcon name="trash-2" size={16}/>Delete</button>{/if}
    {:else if mode === 'rename'}
      <header><strong>Rename</strong><PopupClose label="Close rename" disabled={busy} onclick={close} /></header>
      <form onsubmit={event => { event.preventDefault(); void choose('rename') }}>
        <label>Name<input aria-label="Name" maxlength={maxNameLength} bind:value disabled={busy}/></label>
        <div class="buttons"><button type="submit" disabled={busy || !value.trim()}>Save</button></div>
      </form>
    {:else}
      <header><strong>Delete</strong><PopupClose label="Close delete" disabled={busy} onclick={close} /></header>
      <p>Delete “{name}”?</p><p class="muted">Saved files and chat history stay.</p>
      <div class="buttons"><button onclick={() => choose('delete')} disabled={busy}>Confirm delete</button><button onclick={close} disabled={busy}>Cancel</button></div>
    {/if}
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
{/if}
<style>
  header { display:flex; align-items:center; justify-content:space-between; padding:6px; font:var(--text-13) var(--font-human); }
  button { display:flex;align-items:center;gap:8px;border:0;border-radius:var(--radius-control);background:transparent;color:var(--ink);font:var(--text-13) var(--font-human);padding:7px 8px;cursor:pointer; }
  button:hover:not(:disabled), .opened {background:var(--faint)} button:disabled {opacity:.5;cursor:default}
  .catalog-actions {padding:4px;color:var(--muted);flex:none}
  .catalog-menu {position:fixed;z-index:100;width:220px;padding:6px;box-sizing:border-box;}
  .catalog-menu > button {width:100%;text-align:left}
  p {margin:8px;overflow-wrap:anywhere;font:var(--text-13) var(--font-human)} .muted {color:var(--muted)}
  form {padding:6px} label {font:var(--text-13) var(--font-human)} input {box-sizing:border-box;width:100%;margin:6px 0;padding:6px;color:var(--ink);background:var(--surface);border:1px solid var(--border);border-radius:var(--radius-control)} .buttons {display:flex;gap:4px}
</style>

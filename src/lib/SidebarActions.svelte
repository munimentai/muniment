<script>
  // The sidebar's foot: Settings, Usage and Notifications as three icon
  // buttons, each named on hover and to assistive technology with its
  // shortcut. The Notifications badge counts only what needs the user.
  // One name shows at a time: it follows the pointer after a short delay,
  // or keyboard focus, and goes when the pointer leaves, focus leaves, a
  // button is pressed or the page changes.
  import { onMount } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { settingsOpen = false, usageOpen = false, notificationsOpen = false, needsYou = 0, settingsShortcut = '', usageShortcut = '', notificationsShortcut = '', settingsButton = $bindable(), disabled = false, onsettings, onusage, onnotifications } = $props()
  const keyLabel = (shortcut) => shortcut.replace('Meta', '⌘').replace('Control', 'Ctrl').replace('Shift', '⇧').replaceAll('+', navigator.platform?.startsWith('Mac') ? '' : '+')
  const count = $derived(needsYou > 99 ? '99+' : String(needsYou))
  const HOVER_MS = 350
  let tip = $state(null)
  let timer
  // Focus that follows a key press names its button; focus a click or the
  // app moves back after a dialog closes does not.
  let keyboard = false

  onMount(() => {
    const key = () => { keyboard = true }
    const pointer = () => { keyboard = false }
    window.addEventListener('keydown', key, true)
    window.addEventListener('pointerdown', pointer, true)
    return () => { window.removeEventListener('keydown', key, true); window.removeEventListener('pointerdown', pointer, true); clearTimeout(timer) }
  })

  // A page that opens or closes hides the name.
  $effect(() => {
    void [settingsOpen, usageOpen, notificationsOpen]
    hide()
  })

  function hide() {
    clearTimeout(timer)
    tip = null
  }

  function hover(event, text) {
    clearTimeout(timer)
    const left = event.currentTarget.offsetLeft
    timer = setTimeout(() => { tip = { text, left } }, HOVER_MS)
  }

  function focus(event, text) {
    if (!keyboard) return
    clearTimeout(timer)
    tip = { text, left: event.currentTarget.offsetLeft }
  }

  const name = (label, shortcut) => `${label} ${keyLabel(shortcut)}`.trim()
</script>

<div class="sidebar-actions" role="toolbar" aria-label="Settings, usage and notifications">
  <button type="button" class="dock-button" aria-label="Settings" aria-haspopup="dialog" aria-expanded={settingsOpen} aria-keyshortcuts={settingsShortcut} bind:this={settingsButton} onpointerenter={(event) => hover(event, name('Settings', settingsShortcut))} onpointerleave={hide} onfocus={(event) => focus(event, name('Settings', settingsShortcut))} onblur={hide} onpointerdown={hide} onclick={(event) => { hide(); onsettings?.(event) }}><LucideIcon name="settings" size={18} /></button>
  <button type="button" class="dock-button" aria-label="Usage" aria-pressed={usageOpen} aria-keyshortcuts={usageShortcut} {disabled} onpointerenter={(event) => hover(event, name('Usage', usageShortcut))} onpointerleave={hide} onfocus={(event) => focus(event, name('Usage', usageShortcut))} onblur={hide} onpointerdown={hide} onclick={(event) => { hide(); onusage?.(event) }}><LucideIcon name="chart-column" size={18} /></button>
  <button type="button" class="dock-button" aria-label={needsYou ? `Notifications, ${needsYou} need you` : 'Notifications'} aria-pressed={notificationsOpen} aria-keyshortcuts={notificationsShortcut} {disabled} onpointerenter={(event) => hover(event, name('Notifications', notificationsShortcut))} onpointerleave={hide} onfocus={(event) => focus(event, name('Notifications', notificationsShortcut))} onblur={hide} onpointerdown={hide} onclick={(event) => { hide(); onnotifications?.(event) }}>
    <LucideIcon name="bell" size={18} />
    {#if needsYou}<span class="badge" aria-hidden="true">{count}</span>{/if}
  </button>
  {#if tip}<span class="dock-tip" role="tooltip" style:left={`${tip.left}px`}>{tip.text}</span>{/if}
</div>

<style>
  .sidebar-actions { position: relative; display: flex; align-items: center; gap: 2px; padding: 4px 6px; }
  .dock-button { position: relative; display: inline-grid; place-items: center; width: 32px; height: 32px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .dock-button :global(.lucide) { color: inherit; }
  .dock-button:hover:not(:disabled), .dock-button[aria-pressed="true"], .dock-button[aria-expanded="true"] { background: var(--faint); color: var(--ink); }
  .dock-button:disabled { opacity: .5; cursor: default; }
  .badge { position: absolute; top: 1px; right: 0; min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; border-radius: var(--radius-control); background: var(--warning); color: var(--paper); font: 600 var(--text-12) var(--font-mono); line-height: 16px; text-align: center; }
  .dock-tip { position: absolute; bottom: calc(100% + 2px); z-index: 4; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
</style>

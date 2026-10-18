<script>
  // The sidebar's foot: Settings, Usage and Notifications as three icon
  // buttons, each named on hover and to assistive technology with its
  // shortcut. The Notifications badge counts only what needs the user.
  import LucideIcon from './LucideIcon.svelte'
  let { settingsOpen = false, usageOpen = false, notificationsOpen = false, needsYou = 0, settingsShortcut = '', usageShortcut = '', notificationsShortcut = '', settingsButton = $bindable(), disabled = false, onsettings, onusage, onnotifications } = $props()
  const keyLabel = (shortcut) => shortcut.replace('Meta', '⌘').replace('Control', 'Ctrl').replace('Shift', '⇧').replaceAll('+', navigator.platform?.startsWith('Mac') ? '' : '+')
  const count = $derived(needsYou > 99 ? '99+' : String(needsYou))
</script>

<div class="sidebar-actions" role="toolbar" aria-label="Settings, usage and notifications">
  <button type="button" class="dock-button" aria-label="Settings" aria-haspopup="dialog" aria-expanded={settingsOpen} aria-keyshortcuts={settingsShortcut} data-tooltip={`Settings ${keyLabel(settingsShortcut)}`} bind:this={settingsButton} onclick={onsettings}><LucideIcon name="settings" size={18} /></button>
  <button type="button" class="dock-button" aria-label="Usage" aria-pressed={usageOpen} aria-keyshortcuts={usageShortcut} data-tooltip={`Usage ${keyLabel(usageShortcut)}`} {disabled} onclick={onusage}><LucideIcon name="chart-column" size={18} /></button>
  <button type="button" class="dock-button" aria-label={needsYou ? `Notifications, ${needsYou} need you` : 'Notifications'} aria-pressed={notificationsOpen} aria-keyshortcuts={notificationsShortcut} data-tooltip={`Notifications ${keyLabel(notificationsShortcut)}`} {disabled} onclick={onnotifications}>
    <LucideIcon name="bell" size={18} />
    {#if needsYou}<span class="badge" aria-hidden="true">{count}</span>{/if}
  </button>
</div>

<style>
  .sidebar-actions { display: flex; align-items: center; gap: 2px; padding: 4px 6px; }
  .dock-button { position: relative; display: inline-grid; place-items: center; width: 32px; height: 32px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .dock-button :global(.lucide) { color: inherit; }
  .dock-button:hover:not(:disabled), .dock-button[aria-pressed="true"], .dock-button[aria-expanded="true"] { background: var(--faint); color: var(--ink); }
  .dock-button:disabled { opacity: .5; cursor: default; }
  .badge { position: absolute; top: 1px; right: 0; min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box; border-radius: var(--radius-control); background: var(--warning); color: var(--paper); font: 600 var(--text-12) var(--font-mono); line-height: 16px; text-align: center; }
  .dock-button::after { content: attr(data-tooltip); position: absolute; left: 0; bottom: calc(100% + 6px); z-index: 4; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; opacity: 0; visibility: hidden; transition: opacity 80ms ease; }
  .dock-button:hover::after, .dock-button:focus-visible::after { opacity: 1; visibility: visible; transition-delay: 350ms; }
  @media (prefers-reduced-motion: reduce) { .dock-button::after { transition: none; } }
</style>

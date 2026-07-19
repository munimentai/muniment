<script>
  import { onMount } from 'svelte'

  import { accessErrorState, accessIdleState, accessLoadingState, accessReadyState, devicesErrorState, devicesIdleState, devicesLoadingState, devicesReadyState } from './auth-state.js'

  let { tauri, subject, onSignOut, escapeBlocked = () => false } = $props()
  let access = $state(accessIdleState)
  let devices = $state(devicesIdleState)
  let profileSnapshot = $state(null)
  let accessOpen = $state(false)
  let expandedGroups = $state(new Set())
  let profileButton = $state()
  let accessPopover = $state()

  async function loadAccess(open = false) {
    if (open) accessOpen = true
    access = accessLoadingState()
    expandedGroups = new Set()
    if (open) requestAnimationFrame(() => accessPopover?.focus())
    try {
      const snapshot = await tauri.invoke('auth_entitlement_snapshot')
      profileSnapshot = snapshot
      access = accessReadyState(snapshot)
    } catch (err) {
      access = accessErrorState(err)
    }
  }

  function openAccess() {
    loadAccess(true)
    loadDevices()
  }

  async function loadDevices() {
    devices = devicesLoadingState()
    try {
      devices = devicesReadyState(await tauri.invoke('auth_devices'))
    } catch (_) {
      devices = devicesErrorState()
    }
  }

  function lastActive(value) {
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value))
  }

  function closeAccess() {
    if (!accessOpen) return
    accessOpen = false
    profileButton?.focus()
  }

  function toggleGroup(index) {
    const next = new Set(expandedGroups)
    next.has(index) ? next.delete(index) : next.add(index)
    expandedGroups = next
  }

  onMount(() => {
    loadAccess()
    const outside = (event) => {
      if (accessOpen && !accessPopover?.contains(event.target) && !profileButton?.contains(event.target)) closeAccess()
    }
    const escape = (event) => {
      if (accessOpen && event.key === 'Escape' && !escapeBlocked()) {
        event.preventDefault()
        closeAccess()
      }
    }
    document.addEventListener('click', outside)
    document.addEventListener('keydown', escape)
    return () => {
      document.removeEventListener('click', outside)
      document.removeEventListener('keydown', escape)
    }
  })
</script>

<div class="profile-block">
  <button bind:this={profileButton} class="profile-button" aria-haspopup="dialog" aria-expanded={accessOpen} onclick={() => accessOpen ? closeAccess() : openAccess()}><span><strong>{profileSnapshot?.user_display_name ?? subject ?? 'Signed in'}</strong><small>{profileSnapshot ? `${profileSnapshot.organization_display_name ?? profileSnapshot.org_id} · ${profileSnapshot.role}` : 'Access unavailable'}</small></span></button>
  {#if accessOpen}
    <div bind:this={accessPopover} class="access-popover" role="dialog" aria-label="Your access" tabindex="-1">
      <header><div><h2>Your access</h2>{#if access.name === 'ready'}<p>Snapshot v{access.snapshot.snapshot_version}</p>{/if}</div><button class="quiet close-access" aria-label="Close your access" onclick={closeAccess}>×</button></header>
      {#if access.name === 'loading'}
        <p class="access-status" aria-live="polite">Checking your current access…</p>
      {:else if access.name === 'error'}
        <div class="access-status" role="alert"><p>Your access could not be loaded.</p><button onclick={openAccess}>Try again</button></div>
      {:else if access.name === 'ready'}
        <p class="access-label">Your groups</p>
        {#if access.groups.length === 0}<p class="empty-grant">No groups granted</p>{/if}
        {#each access.groups as group, index}
          {@const open = expandedGroups.has(index)}
          <div class="access-group">
            <button class="group-toggle" aria-expanded={open} aria-controls={`access-group-${index}`} onclick={() => toggleGroup(index)}><span>{group.name}</span><span aria-hidden="true">{open ? '−' : '+'}</span></button>
            {#if open}<div class="grant-grid" id={`access-group-${index}`}>
              {#each [['Models', group.models], ['Connections', group.connections], ['Capabilities', group.capabilities]] as category}
                <div><h3>{category[0]}</h3>{#if category[1].length}<ul>{#each category[1] as item}<li>{item}</li>{/each}</ul>{:else}<p class="empty-grant">None granted</p>{/if}</div>
              {/each}
            </div>{/if}
          </div>
        {/each}
      {/if}
      <section class="devices-section" aria-labelledby="devices-heading">
        <h3 id="devices-heading" class="access-label">Devices</h3>
        {#if devices.name === 'loading'}
          <p class="access-status" aria-live="polite">Loading devices…</p>
        {:else if devices.name === 'error'}
          <div class="access-status" role="alert"><p>Devices could not be loaded.</p><button onclick={loadDevices}>Try again</button></div>
        {:else if devices.name === 'ready'}
          {#if devices.devices.length === 0}<p class="empty-grant">No devices found</p>{/if}
          <ul class="device-list">
            {#each devices.devices as device (device.device_id)}
              <li class:revoked={device.revoked_at}>
                <div class="device-heading"><strong>{device.platform}</strong>{#if device.current}<span class="current-device">This device</span>{/if}<span class="device-state">{device.revoked_at ? 'Revoked' : 'Active'}</span></div>
                <time datetime={device.last_active_at}>Last active {lastActive(device.last_active_at)}</time>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
      <footer>Access is set by your admins.</footer>
      <button class="quiet sign-out" onclick={() => { closeAccess(); onSignOut() }}>Sign out</button>
    </div>
  {/if}
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { border-color: var(--muted); }
  button:focus-visible { outline: 2px solid var(--signal); outline-offset: 1px; }
  .profile-block { position: relative; margin-top: auto; padding-top: 10px; border-top: 1px solid var(--border); }
  .profile-button { width: 100%; display: flex; align-items: center; gap: 9px; padding: 9px 8px; border-color: transparent; background: transparent; text-align: left; }
  .profile-button > span:last-child { min-width: 0; display: grid; }
  .profile-button strong { overflow: hidden; text-overflow: ellipsis; font-size: var(--text-13); }
  .profile-button small { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-popover { position: absolute; z-index: 5; left: 0; bottom: calc(100% + 8px); width: 330px; max-height: min(560px, 70vh); overflow-y: auto; padding: 14px; background: var(--paper); border: 1px solid var(--border); border-radius: 10px; box-shadow: 0 12px 36px color-mix(in srgb, var(--ink) 14%, transparent); outline: none; }
  .access-popover:focus-visible { border-color: var(--muted); }
  .access-popover header { display: flex; align-items: start; justify-content: space-between; margin-bottom: 14px; }
  .access-popover h2 { margin: 0; font-size: var(--text-13); }
  .access-popover header p, .access-label { margin: 3px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .close-access { padding: 0 4px; font-size: 18px; }
  .access-label { margin: 0 0 6px; text-transform: uppercase; letter-spacing: .04em; }
  .access-group { border-top: 1px solid var(--border); }
  .group-toggle { width: 100%; display: flex; justify-content: space-between; padding: 9px 2px; border: 0; background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); text-align: left; }
  .grant-grid { display: grid; gap: 10px; padding: 1px 2px 12px 12px; }
  .grant-grid h3 { margin: 0 0 3px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .grant-grid ul { margin: 0; padding: 0; list-style: none; font: var(--text-12) var(--font-mono); }
  .grant-grid li + li { margin-top: 2px; }
  .empty-grant, .access-status { margin: 8px 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-status p { margin: 0 0 6px; }
  .devices-section { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .device-list { margin: 0; padding: 0; list-style: none; }
  .device-list li { padding: 9px 2px; border-top: 1px solid var(--border); }
  .device-list li:first-child { border-top: 0; }
  .device-heading { display: flex; align-items: center; gap: 7px; font-size: var(--text-12); text-transform: capitalize; }
  .device-heading strong { font-weight: 600; }
  .current-device { padding: 1px 5px; border: 1px solid var(--signal); border-radius: 6px; color: var(--signal); font: 10px var(--font-mono); text-transform: none; }
  .device-state { margin-left: auto; color: var(--signal); font: var(--text-12) var(--font-mono); text-transform: none; }
  .device-list .revoked .device-state { color: var(--muted); }
  .device-list time { display: block; margin-top: 3px; color: var(--muted); font: 11px var(--font-mono); }
  .access-popover footer { margin: 12px -14px 0; padding: 11px 14px 0; border-top: 1px solid var(--border); color: var(--muted); font-size: 11px; }
  .sign-out { margin-top: 8px; padding: 2px 0; color: var(--muted); }
  .quiet { background: transparent; border-color: transparent; }
</style>

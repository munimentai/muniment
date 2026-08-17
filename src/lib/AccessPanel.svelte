<script>
  import { onDestroy, onMount, tick } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'

  import { accessErrorState, accessIdleState, accessLoadingState, accessReadyState, companionsErrorState, companionsIdleState, companionsLoadingState, companionsReadyState, devicesErrorState, devicesIdleState, devicesLoadingState, devicesReadyState } from './auth-state.js'
  import { shortcutFromKeyboardEvent } from './dictation-state.js'
  import { THEME_STORAGE_KEY, parseTheme, serializeTheme } from './theme-state.js'

  let { tauri, subject, onSignOut, escapeBlocked = () => false, voiceShortcut, voiceShortcutChanging, onVoiceShortcutChange, defaultVoiceShortcut } = $props()
  let access = $state(accessIdleState)
  let devices = $state(devicesIdleState)
  let companions = $state(companionsIdleState)
  let attachListener = $state({ started: false, failure: null, pending: true, stopped: false })
  let profileSnapshot = $state(null)
  let accessOpen = $state(false)
  let expandedGroups = $state(new Set())
  let profileButton = $state()
  let accessPopover = $state()
  let capturingShortcut = $state(false)
  let pendingShortcut = $state('')
  let shortcutStatus = $state('')
  let theme = $state(readTheme())
  let revokingIdentity = $state(null)
  let revokePending = $state(false)
  let revokeError = $state('')
  let retentionChoice = $state('keep_every_thread')
  let retentionSaving = $state(false)
  let retentionRequest = 0
  let attachListenerPoll = 0
  let profileName = $derived(profileSnapshot?.user_display_name ?? subject ?? 'Signed in')
  let profileDetails = $derived(profileSnapshot ? `${profileSnapshot.organization_display_name ?? profileSnapshot.org_id} · ${profileSnapshot.role}` : 'Access unavailable')

  const themeOptions = [['System', 'system'], ['Light', 'light'], ['Dark', 'dark']]
  const retentionOptions = [
    ['Keep every thread', 'keep_every_thread'],
    ['Delete after 30 days', 'delete_after_30_days'],
    ['Delete after 90 days', 'delete_after_90_days'],
    ['Delete after 1 year', 'delete_after_1_year'],
  ]

  onDestroy(() => { attachListenerPoll += 1 })

  function readTheme() {
    try {
      return parseTheme(localStorage.getItem(THEME_STORAGE_KEY))
    } catch (_) {
      return parseTheme(null)
    }
  }

  function chooseTheme(choice) {
    theme = parseTheme(choice)
    if (theme === 'system') delete document.documentElement.dataset.theme
    else document.documentElement.dataset.theme = theme
    try { localStorage.setItem(THEME_STORAGE_KEY, serializeTheme(theme)) } catch (_) {}
  }

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
    loadRetentionChoice()
    loadDevices()
    loadCompanions()
  }

  async function loadRetentionChoice() {
    const request = ++retentionRequest
    try {
      const choice = await tauri.invoke('thread_retention_choice')
      if (request === retentionRequest) retentionChoice = choice ?? 'keep_every_thread'
    } catch (_) {
      if (request === retentionRequest) retentionChoice = 'keep_every_thread'
    }
  }

  async function chooseRetention(choice) {
    const request = ++retentionRequest
    retentionSaving = true
    try {
      await tauri.invoke('record_thread_retention_choice', { choice })
      if (request === retentionRequest) retentionChoice = choice
    } catch (_) {
    } finally {
      if (request === retentionRequest) retentionSaving = false
    }
  }

  async function loadDevices() {
    devices = devicesLoadingState()
    try {
      devices = devicesReadyState(await tauri.invoke('auth_devices'))
    } catch (_) {
      devices = devicesErrorState()
    }
  }

  async function loadCompanions() {
    companions = companionsLoadingState()
    const poll = ++attachListenerPoll
    try {
      const [programs] = await Promise.all([
        tauri.invoke('attach_companions'),
        waitForAttachListener(poll),
      ])
      if (poll !== attachListenerPoll) return
      companions = companionsReadyState(programs)
    } catch (_) {
      if (poll !== attachListenerPoll) return
      companions = companionsErrorState()
    }
  }

  async function waitForAttachListener(poll) {
    while (true) {
      const status = await tauri.invoke('attach_listener_status')
      if (poll !== attachListenerPoll) return
      attachListener = status
      if (!status.pending) {
        if (status.failure === 'instance_lock') void watchForApprovalPresenter(poll)
        return
      }
      await new Promise((resolve) => setTimeout(resolve, 50))
    }
  }

  async function watchForApprovalPresenter(poll) {
    while (accessOpen && poll === attachListenerPoll) {
      await new Promise((resolve) => setTimeout(resolve, 50))
      if (!accessOpen || poll !== attachListenerPoll) return
      try {
        const status = await tauri.invoke('attach_listener_status')
        if (poll !== attachListenerPoll) return
        attachListener = status
        if (status.failure !== 'instance_lock') return
      } catch (_) {
        return
      }
    }
  }

  function restartMuniment() {
    return tauri.invoke('restart_muniment')
  }

  function closeWindow() {
    return getCurrentWindow().close()
  }

  function askToRevokeCompanion(identity) {
    revokingIdentity = identity
    revokeError = ''
  }

  function cancelRevokeCompanion() {
    if (revokePending) return
    const identity = revokingIdentity
    revokingIdentity = null
    revokeError = ''
    void tick().then(() => Array.from(accessPopover?.querySelectorAll('[data-revoke-companion]') ?? [])
      .find((button) => button.dataset.revokeCompanion === identity)?.focus())
  }

  function revokeConfirmKeydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    cancelRevokeCompanion()
  }

  async function revokeCompanion(identity) {
    if (revokePending) return
    revokePending = true
    revokeError = ''
    try {
      await tauri.invoke('attach_revoke_companion', { clientIdentity: identity })
      revokingIdentity = null
      await loadCompanions()
    } catch (_) {
      revokeError = 'The program could not be revoked.'
    } finally {
      revokePending = false
    }
  }

  function lastActive(value) {
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value))
  }

  function closeAccess() {
    if (!accessOpen) return
    accessOpen = false
    attachListenerPoll += 1
    capturingShortcut = false
    pendingShortcut = ''
    shortcutStatus = ''
    revokingIdentity = null
    revokeError = ''
    profileButton?.focus()
  }

  function captureShortcut(event) {
    if (!capturingShortcut) return
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Escape') {
      capturingShortcut = false
      pendingShortcut = ''
      return
    }
    const shortcut = shortcutFromKeyboardEvent(event)
    if (shortcut) {
      pendingShortcut = shortcut
      shortcutStatus = ''
    } else if (!['Alt', 'Control', 'Meta', 'Shift'].includes(event.key)) shortcutStatus = 'Include at least one modifier key.'
  }

  async function applyShortcut(shortcut) {
    shortcutStatus = ''
    const result = await onVoiceShortcutChange(shortcut)
    if (result) {
      capturingShortcut = false
      pendingShortcut = ''
    } else if (result === null) shortcutStatus = 'The shortcut could not be updated. Voice remains available from the button.'
    else shortcutStatus = 'That shortcut is unavailable. Your previous shortcut still works.'
  }

  function toggleGroup(index) {
    const next = new Set(expandedGroups)
    next.has(index) ? next.delete(index) : next.add(index)
    expandedGroups = next
  }

  onMount(() => {
    loadAccess()
    const outside = (event) => {
      const path = event.composedPath()
      if (accessOpen && !path.includes(accessPopover) && !path.includes(profileButton)) closeAccess()
    }
    const escape = (event) => {
      if (!accessOpen || event.key !== 'Escape') return
      if (revokingIdentity) {
        event.preventDefault()
        event.stopPropagation()
        cancelRevokeCompanion()
        return
      }
      if (!escapeBlocked()) {
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
  <button bind:this={profileButton} class="profile-button" title={profileDetails} aria-haspopup="dialog" aria-expanded={accessOpen} onclick={() => accessOpen ? closeAccess() : openAccess()}><span><strong>{profileName}</strong><small>{profileDetails}</small></span></button>
  {#if accessOpen}
    <div bind:this={accessPopover} class="access-popover" role="dialog" aria-label="Profile" tabindex="-1">
      <header><div><h2>{profileName}</h2><p>{profileDetails}</p></div><button class="quiet close-access" aria-label="Close profile" onclick={closeAccess}>×</button></header>
      <div class="access-content">
        <section aria-labelledby="appearance-heading">
          <h3 id="appearance-heading" class="access-label">Appearance</h3>
          <div class="theme-options" role="group" aria-labelledby="appearance-heading">
            {#each themeOptions as option}
              <button aria-pressed={theme === option[1]} onclick={() => chooseTheme(option[1])}>{option[0]}</button>
            {/each}
          </div>
        </section>
        <section class="retention-section" aria-labelledby="retention-heading">
          <h3 id="retention-heading" class="access-label">Thread retention</h3>
          <p class="retention-help">Choose how long Muniment keeps completed threads.</p>
          <div class="retention-options">
            {#each retentionOptions as option}
              <label><input type="radio" name="thread-retention" value={option[1]} checked={retentionChoice === option[1]} disabled={retentionSaving} onchange={() => chooseRetention(option[1])} /> <span>{option[0]}</span></label>
            {/each}
          </div>
        </section>
        <section class="entitlements-section" aria-labelledby="entitlements-heading">
          <div class="access-heading"><h3 id="entitlements-heading" class="access-label">Your access</h3>{#if access.name === 'ready'}<p>Snapshot v{access.snapshot.snapshot_version}</p>{/if}</div>
          {#if access.name === 'loading'}
            <p class="access-status" aria-live="polite">Checking your current access…</p>
          {:else if access.name === 'error'}
            <div class="access-status" role="alert"><p>Your access could not be loaded.</p><button onclick={openAccess}>Try again</button></div>
          {:else if access.name === 'ready'}
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
          <p class="access-note">Access is set by your admins.</p>
        </section>
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
        <section class="companions-section" aria-labelledby="companions-heading">
          <h3 id="companions-heading" class="access-label">Connected programs</h3>
          {#if companions.name === 'loading'}
            <p class="access-status" aria-live="polite">Loading connected programs…</p>
          {:else if companions.name === 'error'}
            <div class="access-status" role="alert"><p>Connected programs could not be loaded.</p><button onclick={loadCompanions}>Try again</button></div>
          {:else if companions.name === 'ready'}
            {#if !attachListener.started && attachListener.failure === 'filesystem'}
              <div class="access-status" role="alert"><p>The connected programs folder is unavailable.</p><button onclick={restartMuniment}>Restart Muniment</button></div>
            {:else if !attachListener.started && attachListener.failure === 'instance_lock'}
              {#if attachListener.presenting || attachListener.supervisor_running}
                <div class="access-status" role="status"><p>The Muniment background service manages connected programs.</p></div>
              {:else}
                <div class="access-status" role="status"><p>Connected programs are available in another Muniment window.</p><button onclick={closeWindow}>Close this window</button></div>
              {/if}
            {:else if !attachListener.started && attachListener.failure === 'bind'}
              <div class="access-status" role="alert"><p>The connected programs connection could not start.</p><button onclick={restartMuniment}>Restart Muniment</button></div>
            {:else if attachListener.stopped}
              <div class="access-status" role="status"><p>The connected programs listener stopped.</p><button onclick={restartMuniment}>Restart Muniment</button></div>
            {:else if companions.companions.length === 0}<p class="empty-grant">No connected programs found</p>{/if}
            <ul class="companion-list">
              {#each companions.companions as companion (companion.identity)}
                <li class="companion-row">
                  {#if revokingIdentity === companion.identity}
                    <div class="companion-confirm" role="group" aria-label={`Revoke ${companion.claimed_kind}?`}>
                      <p><strong>{companion.claimed_kind}</strong> must be approved again before it can reconnect.</p>
                      {#if revokeError}<p class="revoke-error" role="alert">{revokeError}</p>{/if}
                      <div class="companion-actions">
                        <button type="button" disabled={revokePending} onclick={() => revokeCompanion(companion.identity)} onkeydown={revokeConfirmKeydown}>{revokeError ? 'Retry revoke' : 'Revoke'}</button>
                        <button type="button" disabled={revokePending} onclick={cancelRevokeCompanion} onkeydown={revokeConfirmKeydown}>Cancel</button>
                      </div>
                    </div>
                  {:else}
                    <div class="companion-heading"><strong title={companion.claimed_kind}>{companion.claimed_kind}</strong><span>Claimed kind</span></div>
                    <p class="companion-version" title={companion.claimed_version}>Claimed version: {companion.claimed_version}</p>
                    {#if companion.approved_at}<time datetime={companion.approved_at}>Approved {lastActive(companion.approved_at)}</time>{:else}<p class="companion-time">Approval time unavailable</p>{/if}
                    <button type="button" class="companion-revoke" data-revoke-companion={companion.identity} aria-label={`Revoke ${companion.claimed_kind}`} onclick={() => askToRevokeCompanion(companion.identity)}>Revoke</button>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </section>
        <section class="voice-section" aria-labelledby="voice-heading">
          <h3 id="voice-heading" class="access-label">Voice shortcut</h3>
          <p class="shortcut-help">Hold this shortcut to dictate from anywhere.</p>
          <button class="shortcut-capture" aria-label={capturingShortcut ? 'Record voice shortcut' : `Change voice shortcut, current ${voiceShortcut}`} onclick={() => { capturingShortcut = true; pendingShortcut = ''; shortcutStatus = '' }} onkeydown={captureShortcut} disabled={voiceShortcutChanging}>
            <span>{capturingShortcut ? pendingShortcut || 'Press a shortcut…' : voiceShortcut}</span><small>{capturingShortcut ? 'Modifier + key' : 'Change'}</small>
          </button>
          {#if capturingShortcut}
            <div class="shortcut-actions"><button onclick={() => { capturingShortcut = false; pendingShortcut = ''; shortcutStatus = '' }}>Cancel</button><button onclick={() => applyShortcut(pendingShortcut)} disabled={!pendingShortcut || voiceShortcutChanging}>{voiceShortcutChanging ? 'Applying…' : 'Apply'}</button></div>
          {/if}
          <button class="quiet restore-shortcut" onclick={() => applyShortcut(defaultVoiceShortcut)} disabled={voiceShortcut === defaultVoiceShortcut || voiceShortcutChanging}>Restore default</button>
          {#if shortcutStatus}<p class="shortcut-error" role="alert">{shortcutStatus}</p>{/if}
        </section>
      </div>
      <footer class="access-footer"><button class="quiet sign-out" onclick={() => { closeAccess(); onSignOut() }}>Sign out</button></footer>
    </div>
  {/if}
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { border-color: var(--muted); }
  .profile-block { position: relative; margin-top: auto; padding-top: 10px; border-top: 1px solid var(--border); }
  .profile-button { width: 100%; display: flex; align-items: center; gap: 9px; padding: 9px 8px; border-color: transparent; background: transparent; text-align: left; }
  .profile-button > span:last-child { min-width: 0; display: grid; }
  .profile-button strong { overflow: hidden; text-overflow: ellipsis; font-size: var(--text-13); }
  .profile-button small { overflow: hidden; color: var(--muted); font: var(--text-12) var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  .access-popover { position: absolute; z-index: 5; left: 0; bottom: calc(100% + 8px); width: 330px; max-height: calc(100vh - 82px); display: flex; flex-direction: column; overflow: hidden; background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); box-shadow: var(--shadow-overlay); outline: none; }
  .access-popover:focus-visible { border-color: var(--muted); }
  .access-popover header { display: flex; flex: none; align-items: start; justify-content: space-between; padding: 14px; border-bottom: 1px solid var(--border); }
  .access-popover h2 { margin: 0; font-size: var(--text-13); }
  .access-popover header p, .access-label { margin: 3px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-heading { display: flex; align-items: baseline; justify-content: space-between; }
  .access-heading p { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .close-access { min-width: 24px; min-height: 24px; padding: 0 4px; font-size: var(--text-17); }
  .access-content { min-height: 0; overflow-y: auto; padding: 14px; }
  .access-label { margin: 0 0 6px; text-transform: uppercase; letter-spacing: .04em; }
  .access-group { border-top: 1px solid var(--border); }
  .group-toggle { width: 100%; display: flex; justify-content: space-between; padding: 9px 2px; border: 0; background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); text-align: left; }
  .grant-grid { display: grid; gap: 10px; padding: 1px 2px 12px 12px; }
  .grant-grid h3 { margin: 0 0 3px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .grant-grid ul { margin: 0; padding: 0; list-style: none; font: var(--text-12) var(--font-mono); }
  .grant-grid li + li { margin-top: 2px; }
  .empty-grant, .access-status { margin: 8px 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-status p { margin: 0 0 6px; }
  .retention-section, .entitlements-section { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .retention-help { margin: 0 0 5px; color: var(--muted); font-size: var(--text-12); }
  .retention-options { display: grid; }
  .retention-options label { display: flex; min-height: 24px; align-items: center; gap: 7px; font-size: var(--text-12); cursor: pointer; }
  .retention-options input { min-width: 24px; min-height: 24px; margin: 0; accent-color: var(--ink); cursor: pointer; }
  .access-note { margin: 10px 0 0; color: var(--muted); font-size: var(--text-12); }
  .devices-section { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .companions-section { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .voice-section { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .theme-options { display: inline-flex; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .theme-options button { position: relative; border: 0; border-radius: 0; background: transparent; color: var(--muted); padding: 5px 12px; }
  .theme-options button + button { border-left: 1px solid var(--border); }
  .theme-options button:first-child { border-radius: var(--radius-control) 0 0 var(--radius-control); }
  .theme-options button:last-child { border-radius: 0 var(--radius-control) var(--radius-control) 0; }
  .theme-options button[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  .theme-options button:focus-visible { z-index: 1; }
  .shortcut-help { margin: 0 0 8px; color: var(--muted); font-size: var(--text-12); }
  .shortcut-capture { width: 100%; display: flex; align-items: center; justify-content: space-between; padding: 8px 9px; font-family: var(--font-mono); text-align: left; }
  .shortcut-capture small { color: var(--muted); font: var(--text-12) var(--font-human); }
  .shortcut-actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 7px; }
  .restore-shortcut { margin-top: 6px; padding: 3px 0; color: var(--muted); }
  .shortcut-error { margin: 5px 0 0; color: var(--oxide); font-size: var(--text-12); }
  .device-list { margin: 0; padding: 0; list-style: none; }
  .device-list li { padding: 9px 2px; border-top: 1px solid var(--border); }
  .device-list li:first-child { border-top: 0; }
  .device-heading { display: flex; align-items: center; gap: 7px; font-size: var(--text-12); }
  .device-heading strong { font-weight: 600; }
  .revoked .device-heading strong { color: var(--oxide); font-weight: 400; text-decoration: line-through; }
  /* §1.2 forbids signal on badges at rest; §1.5 puts chips on radius 2. */
  .current-device { padding: 1px 5px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--faint); color: var(--muted); font: var(--text-12) var(--font-mono); }
  /* §6: the word, not the color, carries Active vs Revoked. */
  .device-state { margin-left: auto; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .device-list time { display: block; margin-top: 3px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .companion-list { margin: 0; padding: 0; list-style: none; }
  .companion-list li { min-width: 0; padding: 9px 2px; border-top: 1px solid var(--border); }
  .companion-list li:first-child { border-top: 0; }
  .companion-row { position: relative; }
  .companion-heading { display: flex; min-width: 0; align-items: center; gap: 7px; font-size: var(--text-12); }
  .companion-heading strong, .companion-version { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .companion-heading strong { min-width: 0; font-weight: 600; }
  .companion-heading span { flex: none; margin-left: auto; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .companion-version, .companion-time, .companion-list time { display: block; margin: 3px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .companion-revoke { position: absolute; top: 5px; right: 0; min-width: 24px; min-height: 24px; padding: 3px 6px; border-color: transparent; background: var(--paper); color: var(--muted); font: var(--text-12) var(--font-mono); opacity: 0; transition: opacity 120ms ease; }
  .companion-row:hover .companion-revoke, .companion-row:focus-within .companion-revoke { opacity: 1; }
  .companion-row:hover .companion-heading span, .companion-row:focus-within .companion-heading span { visibility: hidden; }
  .companion-revoke:hover:not(:disabled) { border-color: transparent; background: var(--faint); color: var(--ink); }
  .companion-revoke:focus-visible, .companion-confirm button:focus-visible { outline-color: var(--ink); }
  .companion-confirm { color: var(--ink); font-size: var(--text-12); }
  .companion-confirm p { margin: 0; }
  .companion-confirm strong { font-weight: 600; }
  .companion-actions { display: flex; justify-content: flex-end; gap: 5px; margin-top: 6px; }
  .companion-actions button { padding: 3px 6px; border-color: transparent; background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); }
  .companion-actions button:hover:not(:disabled) { background: var(--faint); }
  .revoke-error { margin-top: 5px !important; color: var(--muted); }
  .access-footer { flex: none; padding: 9px 14px; border-top: 1px solid var(--border); }
  .sign-out { padding: 2px 0; color: var(--muted); }
  .quiet { background: transparent; border-color: transparent; }
</style>

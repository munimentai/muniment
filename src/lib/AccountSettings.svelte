<script>
  // The account sections of Settings: the person, their plan, the access the
  // server grants them and the programs paired with this desktop. Their
  // devices and phone are on Settings → Machines, and thread retention is on
  // Settings → Data.
  import { onDestroy, onMount, tick } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'

  import { accessErrorState, accessIdleState, accessLoadingState, accessReadyState, companionsErrorState, companionsIdleState, companionsLoadingState, companionsReadyState } from './auth-state.js'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'

  // `children` sits under the signed-in card, such as the MuniCloud vault switch.
  let { tauri, subject = '', onsignout = null, children } = $props()
  let access = $state(accessIdleState)
  let companions = $state(companionsIdleState)
  let attachListener = $state({ started: false, failure: null, pending: true, stopped: false })
  let sectionRoot = $state()
  // The account card reads the entitlement snapshot: the person, their
  // organization and role, and the plan its capabilities imply.
  const snapshot = $derived(access.name === 'ready' ? access.snapshot : null)
  const displayName = $derived(snapshot?.user_display_name ?? (subject || 'Signed in'))
  const orgLine = $derived(snapshot ? [snapshot.organization_display_name ?? snapshot.org_id, snapshot.role].filter(Boolean).join(' · ') : 'Muniment account')
  const initials = $derived(displayName.split(/\s+/).filter(Boolean).slice(0, 2).map((word) => word[0].toUpperCase()).join('') || 'M')
  // Local needs no account, a free account adds the mobile relay, and payment
  // adds sharing, scheduled execution and routing.
  const PLANS = [
    { id: 'local', label: 'Local', summary: 'This desktop with your own providers, no account.', features: ['Chats, agents and artifacts', 'Your models and keys'] },
    { id: 'free', label: 'Free', summary: 'Adds the mobile relay.', features: ['Phone pairing and remote control', 'Devices on one account'] },
    { id: 'pro', label: 'Pro', summary: 'Adds sharing, scheduled execution and routing.', features: ['Shared threads and artifacts', 'Scheduled agents', 'Cloud routing'] },
  ]
  const plan = $derived((access.capabilities ?? []).some((capability) => /share|schedul|rout/i.test(capability)) ? 'pro' : 'free')
  let expandedGrants = $state(new Set())
  let revokingIdentity = $state(null)
  let revokePending = $state(false)
  let revokeError = $state('')
  let attachListenerPoll = 0
  let watching = true

  onDestroy(() => {
    watching = false
    attachListenerPoll += 1
  })

  async function loadAccess() {
    access = accessLoadingState()
    expandedGrants = new Set()
    try {
      access = accessReadyState(await tauri.invoke('auth_entitlement_snapshot'))
    } catch (err) {
      access = accessErrorState(err)
    }
  }

  function reload() {
    loadAccess()
    loadCompanions()
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
    while (watching && poll === attachListenerPoll) {
      await new Promise((resolve) => setTimeout(resolve, 50))
      if (!watching || poll !== attachListenerPoll) return
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
    void tick().then(() => Array.from(sectionRoot?.querySelectorAll('[data-revoke-companion]') ?? [])
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

  function toggleGrant(index) {
    const next = new Set(expandedGrants)
    next.has(index) ? next.delete(index) : next.add(index)
    expandedGrants = next
  }

  onMount(() => {
    reload()
    const escape = (event) => {
      if (event.key !== 'Escape' || !revokingIdentity) return
      event.preventDefault()
      event.stopPropagation()
      cancelRevokeCompanion()
    }
    document.addEventListener('keydown', escape)
    return () => document.removeEventListener('keydown', escape)
  })
</script>

<div class="account-sections" bind:this={sectionRoot}>
  <Group label="Signed-in account">
    <Row title={displayName} description={orgLine}>
      {#snippet lead()}<span class="account-avatar" aria-hidden="true">{initials}</span>{/snippet}
      <span class="plan-chip">{PLANS.find((tier) => tier.id === plan).label}</span>
      {#if onsignout}<Button onclick={onsignout}>Sign out</Button>{/if}
    </Row>
  </Group>
  {@render children?.()}
  <section class="plan-section" aria-labelledby="plan-heading">
    <h3 id="plan-heading" class="settings-heading">Plan</h3>
    <p class="section-help">Plans add what needs a server. Everything local stays free.</p>
    <ul class="plan-tiers">
      {#each PLANS as tier (tier.id)}
        <li class:current={tier.id === plan} aria-current={tier.id === plan ? 'true' : undefined}>
          <div class="plan-name"><strong>{tier.label}</strong>{#if tier.id === plan}<span class="plan-chip">Your plan</span>{/if}</div>
          <p>{tier.summary}</p>
          <ul>{#each tier.features as feature}<li>{feature}</li>{/each}</ul>
        </li>
      {/each}
    </ul>
  </section>
  <section class="companions-section" aria-labelledby="companions-heading">
    <h3 id="companions-heading" class="settings-heading">Connected programs</h3>
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
      {#if companions.companions.length}
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
              <div class="companion-heading"><strong>{companion.claimed_kind}</strong><span>Claimed kind</span></div>
              <p class="companion-version">Claimed version: {companion.claimed_version}</p>
              {#if companion.approved_at}<time datetime={companion.approved_at}>Approved {lastActive(companion.approved_at)}</time>{:else}<p class="companion-time">Approval time unavailable</p>{/if}
              <button type="button" class="companion-revoke" data-revoke-companion={companion.identity} aria-label={`Revoke ${companion.claimed_kind}`} onclick={() => askToRevokeCompanion(companion.identity)}>Revoke</button>
            {/if}
          </li>
        {/each}
      </ul>
      {/if}
    {/if}
  </section>
  <section class="entitlements-section" role="group" aria-labelledby="entitlements-heading">
    <div class="access-heading"><h3 id="entitlements-heading" class="settings-heading">Your access</h3>{#if access.name === 'ready'}<p>Snapshot v{access.snapshot.snapshot_version}</p>{/if}</div>
    {#if access.name === 'loading'}
      <p class="access-status" aria-live="polite">Checking your current access…</p>
    {:else if access.name === 'error'}
      <div class="access-status" role="alert"><p>Your access could not be loaded.</p><button onclick={reload}>Try again</button></div>
    {:else if access.name === 'ready'}
      <div class="access-card">
      <div class="grant-grid">
        <div><h3>Capabilities</h3>{#if access.capabilities.length}<ul>{#each access.capabilities as capability}<li>{capability}</li>{/each}</ul>{:else}<p class="empty-grant">The snapshot lists no capabilities.</p>{/if}</div>
      </div>
      {#if access.grants.length === 0}<p class="empty-grant">The snapshot lists no grants.</p>{/if}
      {#each access.grants as grant, index}
        {@const open = expandedGrants.has(index)}
        <div class="access-group">
          <button class="group-toggle" aria-expanded={open} aria-controls={`access-grant-${index}`} onclick={() => toggleGrant(index)}><span>{grant.effect} {grant.action} · {grant.resource_id ?? grant.resource_type}</span><span aria-hidden="true">{open ? '−' : '+'}</span></button>
          {#if open}<div class="grant-grid" id={`access-grant-${index}`}>
            <div><h3>Principal</h3><p>{grant.principal_type} · {grant.principal_id ?? 'Not specified'}</p></div>
            <div><h3>Resource type</h3><p>{grant.resource_type}</p></div>
            <div><h3>Expiration</h3>{#if grant.expires_at}<time datetime={grant.expires_at}>{lastActive(grant.expires_at)}</time>{:else}<p>The grant has no expiration.</p>{/if}</div>
          </div>{/if}
        </div>
      {/each}
      </div>
    {/if}
    <p class="access-note">Your admins set access. The snapshot is a display hint. The server checks each request.</p>
  </section>
</div>

<style>
  /* Account reads like every Settings page: sections under semibold headings,
     28px apart, with lists in bordered groups instead of dividers between sections. */
  .account-sections { display: grid; gap: var(--page-gap); }
  .account-sections section > .settings-heading, .access-heading { margin: 0 0 4px; }
  .section-help { margin: 0 0 10px; color: var(--muted); font-size: var(--text-13); }
  .plan-section, .companions-section, .entitlements-section { min-width: 0; }
  .account-avatar { display: grid; place-items: center; width: 32px; height: 32px; border-radius: var(--radius-control); background: var(--surface); border: 1px solid var(--border); font: var(--weight-semibold) var(--text-13) var(--font-human); }
  .plan-chip { flex: none; padding: 1px 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .plan-tiers { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .plan-tiers > li { display: grid; align-content: start; gap: 6px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .plan-tiers > li.current { background: var(--faint); }
  .plan-name { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .plan-name strong { font-size: var(--text-15); font-weight: 600; }
  .plan-tiers p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .plan-tiers ul { display: grid; gap: 3px; margin: 4px 0 0; padding: 0; list-style: none; font: var(--text-12) var(--font-mono); color: var(--muted); }
  .account-sections .companion-list { padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  .access-heading { display: flex; align-items: baseline; justify-content: space-between; }
  .access-heading p { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-group { border-top: 1px solid var(--border); }
  .access-card > .grant-grid { padding: 12px 0; }
  .group-toggle { width: 100%; display: flex; justify-content: space-between; gap: 8px; padding: 9px 2px; border: 0; background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); text-align: left; }
  .group-toggle > span:first-child { min-width: 0; overflow-wrap: anywhere; }
  .grant-grid { display: grid; gap: 10px; padding: 1px 2px 12px 12px; }
  .grant-grid h3 { margin: 0 0 3px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .grant-grid ul, .grant-grid p, .grant-grid time { margin: 0; padding: 0; list-style: none; font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .grant-grid li + li { margin-top: 2px; }
  .access-status { margin: 8px 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  /* An empty list reads as one row in its card. */
  .empty-grant { margin: 0; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); color: var(--muted); font-size: var(--text-13); }
  .access-card { padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .access-card .empty-grant { padding: 12px 0; border: 0; border-top: 1px solid var(--border); }
  .access-status p { margin: 0 0 6px; }
  .access-note { margin: 10px 0 0; color: var(--muted); font-size: var(--text-12); }
  .companion-list { margin: 0; padding: 0; list-style: none; }
  .companion-list li { min-width: 0; padding: 10px 0; border-top: 1px solid var(--border); }
  .companion-list li:first-child { border-top: 0; }
  .companion-row { position: relative; }
  .companion-heading { display: flex; min-width: 0; align-items: center; gap: 7px; font-size: var(--text-13); }
  .companion-heading strong, .companion-version { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .companion-heading strong { min-width: 0; font-weight: 600; }
  .companion-heading span { flex: none; margin-left: auto; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .companion-version, .companion-time, .companion-list time { display: block; margin: 3px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .companion-revoke { position: absolute; top: 5px; right: 0; min-width: 24px; min-height: 24px; padding: 3px 6px; border-color: transparent; background: var(--paper); color: var(--muted); font: var(--text-12) var(--font-mono); opacity: 0; transition: opacity 120ms ease; }
  .companion-row:hover .companion-revoke, .companion-row:focus-within .companion-revoke { opacity: 1; }
  .companion-row:hover .companion-heading span, .companion-row:focus-within .companion-heading span { visibility: hidden; }
  .companion-revoke:hover:not(:disabled) { border-color: transparent; background: var(--faint); color: var(--ink); }
  .companion-confirm { color: var(--ink); font-size: var(--text-12); }
  .companion-confirm p { margin: 0; }
  .companion-confirm strong { font-weight: 600; }
  .companion-actions { display: flex; justify-content: flex-end; gap: 5px; margin-top: 6px; }
  .companion-actions button { padding: 3px 6px; border-color: transparent; background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); }
  .companion-actions button:hover:not(:disabled) { background: var(--faint); }
  .revoke-error { margin-top: 5px !important; color: var(--muted); }
</style>

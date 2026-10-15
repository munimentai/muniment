<script>
  // This computer on the Runtimes page: whether other computers may pair with
  // its runtime, the port it listens on, the addresses its links name with
  // their kinds, a one-time pairing link with its QR code, and the devices
  // paired with it. Pairing stays off until the user turns it on. The link
  // holds a secret, so it lives in this page's state and nowhere else.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import { DEFAULT_SSH_PORT, endpointKindLabel, errorLine, scopeLabels, listenPort, seenLabel } from './runtimes.js'

  let { tauri } = $props()
  const id = $props.id()
  let access = $state(null)
  let devices = $state([])
  let link = $state(null)
  let admin = $state(false)
  let port = $state('')
  let addresses = $state('')
  let busy = $state('')
  let error = $state('')
  let copied = $state(false)
  let confirming = $state(null)
  const enabled = $derived(!!access?.settings?.enabled)
  const endpoints = $derived(access?.endpoints ?? [])
  const portValid = $derived(/^\d{1,5}$/.test(port.trim()) && Number(port) >= 1 && Number(port) <= 65535)

  function apply(next) {
    access = next
    port = String(listenPort(next) ?? DEFAULT_SSH_PORT)
    addresses = (next?.settings?.endpoints ?? []).join('\n')
  }

  async function call(name, command, payload) {
    busy = name
    error = ''
    try {
      return await tauri.invoke(command, payload)
    } catch (failure) {
      error = errorLine(failure, 'This computer\'s runtime did not answer.')
      return null
    } finally {
      busy = ''
    }
  }

  async function loadDevices() {
    const listed = await call('devices', 'local_devices')
    if (listed) devices = Array.isArray(listed.devices) ? listed.devices : []
  }

  onMount(() => {
    void call('load', 'local_access_settings').then((next) => { if (next) apply(next) }).then(loadDevices)
    return () => { link = null }
  })

  async function save(nextEnabled, nextPort = null) {
    const endpointsList = addresses.split(/[\s,]+/).map((entry) => entry.trim()).filter(Boolean)
    const next = await call('save', 'local_access_save', { enabled: nextEnabled, port: nextPort, endpoints: endpointsList })
    if (next) apply(next)
    if (!nextEnabled) link = null
  }

  async function createLink() {
    copied = false
    const created = await call('link', 'local_pairing_link', { admin })
    if (created) link = created
  }

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(link.link)
      copied = true
    } catch (_) {
      error = 'The link could not be copied. Select it and copy it.'
    }
  }

  async function revoke(device) {
    if (confirming !== device.device_id) {
      confirming = device.device_id
      return
    }
    const revoked = await call(device.device_id, 'local_device_revoke', { deviceId: device.device_id })
    if (revoked !== null) {
      confirming = null
      await loadDevices()
    }
  }

  const expiry = (value) => {
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? '' : date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }
</script>

<Group heading="This computer" description="Let other computers pair with this one. Pairing stays off until you turn it on.">
  <Row title="Allow pairing" description={enabled ? 'Other computers can reach this runtime with a pairing link.' : 'No other computer can reach this runtime.'}>
    <Toggle checked={enabled} label="Allow pairing" disabled={!access || !!busy} onchange={(next) => save(next)} />
  </Row>
  {#if enabled}
    <form class="local-form" aria-label="Listen port" onsubmit={(event) => { event.preventDefault(); if (portValid) void save(true, Number(port)) }}>
      <div class="field">
        <label for="{id}-port">Listen port</label>
        <input id="{id}-port" class="record" type="text" inputmode="numeric" autocomplete="off" disabled={!!busy} bind:value={port}>
      </div>
      <div class="field wide">
        <label for="{id}-addresses">Addresses links name</label>
        <input id="{id}-addresses" class="record" type="text" autocomplete="off" spellcheck="false" placeholder="Automatic" aria-describedby="{id}-addresses-help" disabled={!!busy} bind:value={addresses}>
        <small id="{id}-addresses-help">Leave empty to list this computer's addresses. Separate host:port entries with spaces.</small>
      </div>
      <Button type="submit" disabled={!portValid || !!busy}>Save</Button>
    </form>
    {#if access?.listening}<p class="line">Listening on <span class="record">{access.listening}</span></p>{/if}
    {#if access?.error}<p class="line" role="alert">Pairing cannot listen. {errorLine(access.error)}</p>{/if}
    {#if access?.overridden}<p class="line">This runtime's host overrides some of these settings.</p>{/if}
    {#each endpoints as endpoint (endpoint.endpoint)}
      <Row title={endpoint.endpoint} label={endpoint.endpoint}>
        {#snippet detail()}<p class="line">{endpointKindLabel(endpoint.kind)}</p>{/snippet}
      </Row>
    {:else}
      <p>No address to advertise. Add one above.</p>
    {/each}
  {/if}
</Group>

{#if enabled}
  <Group heading="Pairing link" description="Open the link on the other computer, or scan the code. It works once.">
    <Row title="Allow pairing management" description="The other computer may pair and revoke devices here. Updates are always allowed.">
      <Toggle checked={admin} label="Allow pairing management" disabled={!!busy} onchange={(next) => { admin = next }} />
    </Row>
    <div class="link-actions">
      <Button icon="plus" disabled={!!busy || !endpoints.length} onclick={createLink}>{link ? 'Create another link' : 'Create pairing link'}</Button>
    </div>
    {#if link}
      <div class="pairing-link" aria-label="Pairing link">
        <div class="pairing-qr">{@html link.qrSvg}</div>
        <div class="link-text">
          <input class="record" type="text" readonly aria-label="Link to open on the other computer" value={link.link}>
          <div class="link-row">
            <Button icon={copied ? 'check' : 'copy'} onclick={copyLink}>{copied ? 'Copied' : 'Copy link'}</Button>
            {#if expiry(link.expiresAt)}<p class="line">Works once, until {expiry(link.expiresAt)}.</p>{/if}
          </div>
        </div>
      </div>
    {/if}
  </Group>
{/if}

<Group heading="Paired devices" count={devices.length}>
  {#each devices as device (device.device_id)}
    {#if confirming === device.device_id}
      <Row title="Revoke {device.name}?" description="It loses access at once. Pairing it again takes a new link.">
        <Button disabled={!!busy} onclick={() => revoke(device)}>Revoke</Button>
        <button type="button" class="quiet" onclick={() => { confirming = null }}>Keep</button>
      </Row>
    {:else}
      <Row title={device.name} meta={device.kind} label={device.name}>
        {#snippet detail()}<p class="line">{scopeLabels(device.scopes)} · {seenLabel(device.last_seen_at)}</p>{/snippet}
        <button type="button" class="quiet" disabled={!!busy} aria-label="Revoke {device.name}" onclick={() => revoke(device)}>Revoke</button>
      </Row>
    {/if}
  {:else}
    <p>No device is paired with this computer.</p>
  {/each}
</Group>
{#if error}<p class="line" role="alert">{error}</p>{/if}

<style>
  .local-form { display: grid; grid-template-columns: minmax(0, 120px) minmax(0, 1fr) auto; align-items: end; gap: 12px 16px; padding: 12px 0; }
  .field { display: grid; gap: 6px; min-width: 0; }
  .field > label { color: var(--muted); font-size: var(--text-13); }
  .field input { width: 100%; min-width: 0; }
  .wide { min-width: 0; }
  small { color: var(--muted); font-size: var(--text-12); }
  .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .line { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .line[role="alert"] { color: var(--ink); }
  .link-actions { padding: 10px 0; }
  .pairing-link { display: flex; flex-wrap: wrap; gap: 16px; padding: 12px 0; }
  .pairing-qr { flex: none; width: 168px; }
  .pairing-qr :global(svg) { display: block; width: 100%; height: auto; border-radius: var(--radius-control); }
  .link-text { flex: 1 1 260px; display: grid; align-content: start; gap: 8px; min-width: 0; }
  .link-text input { width: 100%; min-width: 0; }
  .link-row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; }
  .quiet { background: transparent; }
</style>

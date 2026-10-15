<script>
  // This computer on the Machines page. Pair holds a one-time pairing code
  // for another device, with its QR code, link, Copy and countdown, beside
  // the `add` panel. Showing a code turns on connections the first time.
  // Devices using this computer lists each paired device with Revoke, and
  // Network holds the switch, port and addresses. The link holds a secret, so
  // it lives in this page's state and nowhere else.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import DisclosureSummary from '../lib/ui/DisclosureSummary.svelte'
  import { DEFAULT_SSH_PORT, endpointKindLabel, errorLine, kindIcon, listenPort, seenLabel } from './runtimes.js'
  import LucideIcon from '../lib/LucideIcon.svelte'

  let { tauri, add } = $props()
  const id = $props.id()
  let access = $state(null)
  let devices = $state([])
  let link = $state(null)
  let port = $state('')
  let addresses = $state('')
  let busy = $state('')
  let error = $state('')
  let copied = $state(false)
  let confirming = $state(null)
  let now = $state(Date.now())
  const enabled = $derived(!!access?.settings?.enabled)
  const endpoints = $derived(access?.endpoints ?? [])
  const portValid = $derived(/^\d{1,5}$/.test(port.trim()) && Number(port) >= 1 && Number(port) <= 65535)
  const left = $derived(link ? Math.max(0, new Date(link.expiresAt).getTime() - now) : 0)

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
    const ticker = setInterval(() => { now = Date.now() }, 1000)
    return () => { clearInterval(ticker); link = null }
  })

  async function save(nextEnabled, nextPort = null) {
    const endpointsList = addresses.split(/[\s,]+/).map((entry) => entry.trim()).filter(Boolean)
    const next = await call('save', 'local_access_save', { enabled: nextEnabled, port: nextPort, endpoints: endpointsList })
    if (next) apply(next)
    if (!nextEnabled) link = null
    return next
  }

  async function showCode() {
    if (!enabled && !await save(true)) return
    if (!endpoints.length) {
      error = 'This computer has no address to share. Add one under Network.'
      return
    }
    copied = false
    const created = await call('link', 'local_pairing_link', { admin: false })
    if (created) {
      link = created
      now = Date.now()
    }
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

  const countdown = (milliseconds) => `${Math.floor(milliseconds / 60000)}:${String(Math.floor(milliseconds / 1000) % 60).padStart(2, '0')}`
</script>

<Group heading="Pair" plain>
  <div class="pair-grid">
    <section class="panel" aria-labelledby="{id}-share">
      <h3 id="{id}-share">Let another device use this computer</h3>
      {#if link}
        <div class="pairing-link" aria-label="Pairing code">
          <div class="pairing-qr">{@html link.qrSvg}</div>
          <div class="link-text">
            <div class="link-box">
              <input class="record" type="text" readonly aria-label="Pairing link" value={link.link}>
              <Button icon={copied ? 'check' : 'copy'} onclick={copyLink}>{copied ? 'Copied' : 'Copy'}</Button>
            </div>
            <p class="line" role="timer">{left ? `Works once · expires in ${countdown(left)}` : 'Expired. Make a new code.'}</p>
            <div class="link-row">
              <button type="button" class="quiet" disabled={!!busy} onclick={showCode}>New code</button>
              <button type="button" class="quiet" onclick={() => { link = null }}>Stop pairing</button>
            </div>
          </div>
        </div>
      {:else}
        <p class="line">Scan the code or paste the link on the other device.</p>
        <div><Button variant="primary" disabled={!access || !!busy} onclick={showCode}>Show pairing code</Button></div>
      {/if}
    </section>
    {@render add?.()}
  </div>
</Group>

{#if devices.length}
  <Group heading="Devices using this computer" count={devices.length}>
    {#each devices as device (device.device_id)}
      {#if confirming === device.device_id}
        <Row title="Revoke {device.name}?" description="It loses access at once. Pairing it again takes a new code.">
          <Button disabled={!!busy} onclick={() => revoke(device)}>Revoke</Button>
          <button type="button" class="quiet" onclick={() => { confirming = null }}>Keep</button>
        </Row>
      {:else}
        <Row title={device.name} label={device.name}>
          {#snippet lead()}<span class="device-mark" aria-hidden="true"><LucideIcon name={kindIcon({ kind: device.kind })} size={16} variant="action" /></span>{/snippet}
          {#snippet detail()}<p class="line">{seenLabel(device.last_seen_at)}</p>{/snippet}
          <button type="button" class="quiet" disabled={!!busy} aria-label="Revoke {device.name}" onclick={() => revoke(device)}>Revoke</button>
        </Row>
      {/if}
    {/each}
  </Group>
{/if}

<details class="network">
  <DisclosureSummary>Network</DisclosureSummary>
  <div class="network-body">
    <Row title="Accept connections" description={enabled ? 'Paired devices can reach this computer.' : 'No other device can reach this computer.'}>
      <Toggle checked={enabled} label="Accept connections" disabled={!access || !!busy} onchange={(next) => save(next)} />
    </Row>
    <form class="local-form" aria-label="Listen port" onsubmit={(event) => { event.preventDefault(); if (portValid) void save(enabled, Number(port)) }}>
      <div class="field">
        <label for="{id}-port">Port</label>
        <input id="{id}-port" class="record" type="text" inputmode="numeric" autocomplete="off" disabled={!!busy} bind:value={port}>
      </div>
      <div class="field">
        <label for="{id}-addresses">Addresses in the link</label>
        <input id="{id}-addresses" class="record" type="text" autocomplete="off" spellcheck="false" placeholder="Automatic" disabled={!!busy} bind:value={addresses}>
      </div>
      <Button type="submit" disabled={!portValid || !!busy}>Save</Button>
    </form>
    {#if access?.error}<p class="line" role="alert">Connections cannot start. {errorLine(access.error)}</p>{/if}
    {#if access?.overridden}<p class="line">This runtime's host overrides some of these settings.</p>{/if}
    {#each endpoints as endpoint (endpoint.endpoint)}
      <p class="line"><span class="record">{endpoint.endpoint}</span> · {endpointKindLabel(endpoint.kind)}</p>
    {/each}
  </div>
</details>
{#if error}<p class="line" role="alert">{error}</p>{/if}

<style>
  .pair-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 16px; }
  .pair-grid > :global(.panel), .panel { display: grid; gap: 12px; align-content: start; min-width: 0; padding: 16px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  h3 { margin: 0; font-size: var(--text-13); font-weight: 600; }
  .pairing-link { display: flex; flex-wrap: wrap; gap: 16px; }
  .pairing-qr { flex: none; width: 148px; }
  .pairing-qr :global(svg) { display: block; width: 100%; height: auto; border-radius: var(--radius-control); }
  .link-text { flex: 1 1 160px; display: grid; align-content: start; gap: 10px; min-width: 0; }
  .link-box { display: flex; gap: 8px; min-width: 0; }
  .link-box input { flex: 1; min-width: 0; }
  .link-row { display: flex; flex-wrap: wrap; gap: 4px 12px; }
  .device-mark { display: grid; place-items: center; width: 32px; height: 32px; box-sizing: border-box; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); }
  .network { border-top: 1px solid var(--border); padding-top: 12px; }
  .network-body { display: grid; gap: 10px; max-width: 560px; padding-top: 8px; }
  .local-form { display: grid; grid-template-columns: minmax(0, 100px) minmax(0, 1fr) auto; align-items: end; gap: 12px; }
  .field { display: grid; gap: 6px; min-width: 0; }
  .field > label { color: var(--muted); font-size: var(--text-12); }
  .field input { width: 100%; min-width: 0; }
  .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .line { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .line[role="alert"] { color: var(--ink); }
  .quiet { background: transparent; color: var(--muted); }
  .quiet:hover:not(:disabled) { color: var(--ink); }
</style>

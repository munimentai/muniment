<script>
  // This computer's own settings, under its row on the Machines page: Network
  // holds the switch that lets other machines reach it, the port and the
  // addresses a code names, then Available offline sets what it keeps from
  // the others, and Log lists what happened across your machines.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import { DEFAULT_SSH_PORT, endpointKindLabel, errorLine, listenPort } from './runtimes.js'
  import OfflineSettings from './OfflineSettings.svelte'
  import AuditLog from './AuditLog.svelte'

  let { tauri } = $props()
  const id = $props.id()
  let access = $state(null)
  let port = $state('')
  let addresses = $state('')
  let busy = $state(false)
  let error = $state('')
  const enabled = $derived(!!access?.settings?.enabled)
  const endpoints = $derived(access?.endpoints ?? [])
  const portValid = $derived(/^\d{1,5}$/.test(port.trim()) && Number(port) >= 1 && Number(port) <= 65535)

  function apply(next) {
    access = next
    port = String(listenPort(next) ?? DEFAULT_SSH_PORT)
    addresses = (next?.settings?.endpoints ?? []).join('\n')
  }

  async function call(command, payload) {
    busy = true
    error = ''
    try {
      return await tauri.invoke(command, payload)
    } catch (failure) {
      error = errorLine(failure, 'This computer\'s runtime did not answer.')
      return null
    } finally {
      busy = false
    }
  }

  onMount(() => { void call('local_access_settings').then((next) => { if (next) apply(next) }) })

  async function save(nextEnabled, nextPort = null) {
    const list = addresses.split(/[\s,]+/).map((entry) => entry.trim()).filter(Boolean)
    const next = await call('local_access_save', { enabled: nextEnabled, port: nextPort, endpoints: list })
    if (next) apply(next)
  }
</script>

<Group heading="Network" info="Other machines reach this computer at these addresses.">
  <Row title="Accept connections" description={enabled ? 'Your other machines can reach this computer.' : 'No other machine can reach this computer.'}>
    <Toggle checked={enabled} label="Accept connections" disabled={!access || busy} onchange={(next) => save(next)} />
  </Row>
  <form class="local-form" aria-label="Listen port" onsubmit={(event) => { event.preventDefault(); if (portValid) void save(enabled, Number(port)) }}>
    <div class="field">
      <label for="{id}-port">Port</label>
      <input id="{id}-port" class="record" type="text" inputmode="numeric" autocomplete="off" disabled={busy} bind:value={port}>
    </div>
    <div class="field">
      <label for="{id}-addresses">Addresses in the link</label>
      <input id="{id}-addresses" class="record" type="text" autocomplete="off" spellcheck="false" placeholder="Automatic" disabled={busy} bind:value={addresses}>
    </div>
    <Button type="submit" disabled={!portValid || busy}>Save</Button>
  </form>
  {#if access?.error}<p class="line" role="alert">Connections cannot start. {errorLine(access.error)}</p>{/if}
  {#if access?.overridden}<p class="line">This runtime's host overrides some of these settings.</p>{/if}
  {#each endpoints as endpoint (endpoint.endpoint)}
    <p class="line"><span class="record">{endpoint.endpoint}</span> · {endpointKindLabel(endpoint.kind)}</p>
  {/each}
  {#if error}<p class="line" role="alert">{error}</p>{/if}
</Group>

<OfflineSettings {tauri} />

<AuditLog {tauri} />

<style>
  .local-form { display: grid; grid-template-columns: minmax(0, 100px) minmax(0, 1fr) auto; align-items: end; gap: 12px; padding: 12px 0; }
  .field { display: grid; gap: 6px; min-width: 0; }
  .field > label { color: var(--muted); font-size: var(--text-12); }
  .field input { width: 100%; min-width: 0; }
  .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .line { margin: 0; padding: 8px 0; color: var(--muted); font-size: var(--text-13); }
  .line[role="alert"] { color: var(--ink); }
</style>

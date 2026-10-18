<script>
  // Add machine on the Machines page, both ways in one panel. This computer
  // shows a one-time code, with its QR code, link, Copy and countdown, for
  // the other machine to scan or open, at once when connections are on and
  // after Show code when they are off. Or the other machine shows its code
  // and its link goes in the field here, or an SSH host runs the command that
  // makes one and keeps a tunnel open while it attaches. Either way the
  // machine also joins this computer's machines in the same step: core links
  // a machine that holds admin access, and a device that takes less, such
  // as a phone, stays limited. Showing a code turns on connections the first
  // time. Each link holds a secret, so it lives in this panel's state only.
  import { onMount } from 'svelte'
  import Button from '../lib/ui/Button.svelte'
  import PopupClose from '../lib/PopupClose.svelte'
  import { DEFAULT_SSH_COMMAND, DEFAULT_SSH_PORT, errorLine, listenPort, looksLikePairingLink } from './runtimes.js'

  // `onadded` hears that a machine joined, with the new machine list when
  // this computer added it. `onnetwork` opens this computer's network
  // settings when connections cannot start.
  let { tauri, deviceName = '', onadded = () => {}, onnetwork = () => {}, onclose = () => {} } = $props()
  const id = $props.id()
  let code = $state(null)
  let known = $state(null)
  let now = $state(Date.now())
  let copied = $state(false)
  let mode = $state('link')
  let link = $state('')
  let host = $state('')
  let command = $state(DEFAULT_SSH_COMMAND)
  let port = $state(String(DEFAULT_SSH_PORT))
  let pending = $state('')
  let error = $state('')
  let added = $state('')
  const portValid = $derived(/^\d{1,5}$/.test(port.trim()) && Number(port) >= 1 && Number(port) <= 65535)
  const ready = $derived(mode === 'link' ? looksLikePairingLink(link) : !!host.trim() && !!command.trim() && portValid)
  const left = $derived(code ? Math.max(0, new Date(code.expiresAt).getTime() - now) : 0)
  const countdown = (milliseconds) => `${Math.floor(milliseconds / 60000)}:${String(Math.floor(milliseconds / 1000) % 60).padStart(2, '0')}`

  async function listDevices() {
    const listed = await tauri.invoke('local_devices')
    return Array.isArray(listed?.devices) ? listed.devices : []
  }

  // Connections start the first time a code shows. A link names this
  // computer's addresses, so one with none cannot work.
  async function showCode() {
    pending = 'code'
    error = ''
    copied = false
    try {
      let access = await tauri.invoke('local_access_settings')
      if (!access?.settings?.enabled) {
        access = await tauri.invoke('local_access_save', { enabled: true, port: null, endpoints: access?.settings?.endpoints ?? [] })
      }
      if (access?.error) {
        error = /address already in use/i.test(String(access.error))
          ? `Another program uses port ${listenPort(access) ?? DEFAULT_SSH_PORT}. Choose another port in this computer's network settings.`
          : `Connections cannot start. ${errorLine(access.error)}`
        onnetwork()
        return
      }
      if (!(access?.endpoints ?? []).length) {
        error = 'This computer has no address to share. Add one in its network settings.'
        onnetwork()
        return
      }
      known = new Set((await listDevices().catch(() => [])).map((device) => device.device_id))
      code = await tauri.invoke('local_pairing_link', { admin: true })
      now = Date.now()
    } catch (failure) {
      error = errorLine(failure, 'This computer\'s runtime did not answer.')
    } finally {
      pending = ''
    }
  }

  // A code works once, so a device that was not listed when it showed used it.
  async function watchCode() {
    let devices
    try { devices = await listDevices() } catch (_) { return }
    const used = devices.find((device) => !known?.has(device.device_id))
    if (used && code) {
      code = null
      added = `${used.name} added.`
      onadded(null)
    }
  }

  // A code opens at once only when this computer already accepts connections.
  let accepting = $state(null)
  onMount(() => {
    void tauri.invoke('local_access_settings').then((access) => {
      accepting = !!access?.settings?.enabled
      if (accepting) void showCode()
    }).catch(() => { accepting = false })
    const ticker = setInterval(() => { now = Date.now() }, 1000)
    const watcher = setInterval(() => { if (code && !pending) void watchCode() }, 2000)
    return () => { clearInterval(ticker); clearInterval(watcher); code = null }
  })

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(code.link)
      copied = true
    } catch (_) {
      error = 'The link could not be copied. Select it and copy it.'
    }
  }

  async function add(event) {
    event.preventDefault()
    if (!ready || pending) return
    pending = 'add'
    error = ''
    added = ''
    try {
      const next = mode === 'link'
        ? await tauri.invoke('runtime_pair_link', { link: link.trim(), deviceName: deviceName || null })
        : await tauri.invoke('runtime_pair_ssh', { host: host.trim(), command: command.trim(), port: Number(port), deviceName: deviceName || null })
      link = ''
      host = ''
      added = 'Machine added.'
      onadded(next)
    } catch (failure) {
      error = errorLine(failure, 'The machine was not added. Try again.')
    } finally {
      pending = ''
    }
  }

  function switchMode() {
    mode = mode === 'link' ? 'ssh' : 'link'
    error = ''
    added = ''
  }
</script>

<section class="add-machine" aria-labelledby="{id}-heading">
  <header>
    <h3 id="{id}-heading">Add machine</h3>
    <PopupClose label="Close Add machine" onclick={onclose} />
  </header>
  <div class="ways">
    <div class="way" aria-label="Code from this computer" role="group">
      {#if code}
        <div class="code">
          <div class="qr">{@html code.qrSvg}</div>
          <div class="code-text">
            <p class="line">Scan this code on the other machine, or open the link there.</p>
            <div class="field-row">
              <input class="record" type="text" readonly aria-label="Link from this computer" value={code.link}>
              <Button icon={copied ? 'check' : 'copy'} onclick={copyLink}>{copied ? 'Copied' : 'Copy'}</Button>
            </div>
            <p class="line" role="timer">{left ? `Works once · expires in ${countdown(left)}` : 'Expired. Make a new code.'}</p>
            <div><button type="button" class="quiet" disabled={!!pending} onclick={showCode}>New code</button></div>
          </div>
        </div>
      {:else if pending === 'code'}
        <p class="line" role="status">Making a code…</p>
      {:else}
        <p class="line">{accepting === false ? 'A code lets the other machine reach this computer.' : 'Show a code for the other machine to scan.'}</p>
        <div><Button disabled={!!pending || accepting === null} onclick={showCode}>Show code</Button></div>
      {/if}
    </div>
    <form class="way" aria-label="Link from the other machine" onsubmit={add}>
      <p class="line">Or add the code the other machine shows.</p>
      {#if mode === 'link'}
        <div class="field-row">
          <input class="record" type="text" autocomplete="off" spellcheck="false" placeholder="Paste its link" aria-label="Link from the other machine" disabled={pending === 'add'} bind:value={link}>
          <Button type="submit" disabled={!ready || !!pending}>Add</Button>
        </div>
      {:else}
        <div class="field-row">
          <input class="record" type="text" autocomplete="off" spellcheck="false" placeholder="user@host" aria-label="SSH host" disabled={pending === 'add'} bind:value={host}>
          <Button type="submit" disabled={!ready || !!pending}>Add</Button>
        </div>
        <div class="ssh-options">
          <label>Command <input class="record" type="text" autocomplete="off" spellcheck="false" aria-label="Remote command" disabled={pending === 'add'} bind:value={command}></label>
          <label>Port <input class="record port" type="text" inputmode="numeric" autocomplete="off" aria-label="Runtime port" disabled={pending === 'add'} bind:value={port}></label>
        </div>
      {/if}
      <div class="meta">
        <button type="button" class="quiet" disabled={pending === 'add'} onclick={switchMode}>{mode === 'link' ? 'Use SSH instead' : 'Paste a link instead'}</button>
        {#if pending === 'add'}<p class="line" role="status">Connecting…</p>{/if}
      </div>
    </form>
  </div>
  {#if added}<p class="line good" role="status">{added}</p>{/if}
  {#if error}<p class="line error" role="alert">{error}</p>{/if}
</section>

<style>
  .add-machine { display: grid; gap: 12px; padding: 16px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h3 { margin: 0; font-size: var(--text-15); font-weight: 600; }
  .ways { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px 24px; }
  .way { display: grid; gap: 10px; align-content: start; min-width: 0; }
  .code { display: flex; flex-wrap: wrap; gap: 16px; }
  .qr { flex: none; width: 132px; }
  .qr :global(svg) { display: block; width: 100%; height: auto; border-radius: var(--radius-control); }
  .code-text { flex: 1 1 160px; display: grid; align-content: start; gap: 10px; min-width: 0; }
  .field-row { display: flex; gap: 8px; min-width: 0; }
  .field-row input { flex: 1; min-width: 0; }
  .ssh-options { display: flex; flex-wrap: wrap; gap: 8px 12px; color: var(--muted); font-size: var(--text-12); }
  .ssh-options label { display: inline-flex; align-items: center; gap: 6px; min-width: 0; }
  .ssh-options input { width: 200px; min-width: 0; }
  .ssh-options .port { width: 64px; }
  .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 12px; }
  .line { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .line.good, .line.error { color: var(--ink); }
  .quiet { background: transparent; color: var(--muted); }
  .quiet:hover:not(:disabled) { color: var(--ink); }
</style>

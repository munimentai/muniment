<script>
  // Adds a machine: paste the one-time pairing link it shows, or name an SSH
  // host and the app runs the pairing command there and keeps a tunnel open
  // while it attaches. The link's secret goes to the runtime and nowhere else:
  // the field clears once pairing finishes.
  import Button from '../lib/ui/Button.svelte'
  import { DEFAULT_SSH_COMMAND, DEFAULT_SSH_PORT, errorLine, looksLikePairingLink } from './runtimes.js'

  let { tauri, deviceName = '', onpaired = () => {} } = $props()
  let mode = $state('link')
  let link = $state('')
  let host = $state('')
  let command = $state(DEFAULT_SSH_COMMAND)
  let port = $state(String(DEFAULT_SSH_PORT))
  let pending = $state(false)
  let error = $state('')
  let added = $state('')
  const id = $props.id()
  const portValid = $derived(/^\d{1,5}$/.test(port.trim()) && Number(port) >= 1 && Number(port) <= 65535)
  const ready = $derived(mode === 'link' ? looksLikePairingLink(link) : !!host.trim() && !!command.trim() && portValid)

  async function pair(event) {
    event.preventDefault()
    if (!ready || pending) return
    pending = true
    error = ''
    added = ''
    try {
      const next = mode === 'link'
        ? await tauri.invoke('runtime_pair_link', { link: link.trim(), deviceName: deviceName || null })
        : await tauri.invoke('runtime_pair_ssh', { host: host.trim(), command: command.trim(), port: Number(port), deviceName: deviceName || null })
      link = ''
      host = ''
      added = 'Machine added.'
      onpaired(next)
    } catch (failure) {
      error = errorLine(failure, 'Pairing did not finish. Try again.')
    } finally {
      pending = false
    }
  }

  function switchMode() {
    mode = mode === 'link' ? 'ssh' : 'link'
    error = ''
    added = ''
  }
</script>

<section class="panel" aria-labelledby="{id}-heading">
  <h3 id="{id}-heading">Add a machine</h3>
  <form class="runtime-add" aria-label="Add a machine" onsubmit={pair}>
    {#if mode === 'link'}
      <div class="field-row">
        <input id="{id}-link" class="record" type="text" autocomplete="off" spellcheck="false" placeholder="Paste a pairing link" aria-label="Pairing link" disabled={pending} bind:value={link}>
        <Button type="submit" variant="primary" disabled={!ready || pending}>Add</Button>
      </div>
    {:else}
      <div class="field-row">
        <input id="{id}-host" class="record" type="text" autocomplete="off" spellcheck="false" placeholder="user@host" aria-label="SSH host" disabled={pending} bind:value={host}>
        <Button type="submit" variant="primary" disabled={!ready || pending}>Add</Button>
      </div>
      <div class="ssh-options">
        <label>Command <input class="record" type="text" autocomplete="off" spellcheck="false" aria-label="Remote command" disabled={pending} bind:value={command}></label>
        <label>Port <input class="record port" type="text" inputmode="numeric" autocomplete="off" aria-label="Runtime port" disabled={pending} bind:value={port}></label>
      </div>
    {/if}
  </form>
  <div class="meta">
    <button type="button" class="quiet" disabled={pending} onclick={switchMode}>{mode === 'link' ? 'Use SSH instead' : 'Use a pairing link instead'}</button>
    {#if pending}<p class="state" role="status">Connecting…</p>{:else if added}<p class="state good" role="status">{added}</p>{/if}
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .panel { display: grid; gap: 12px; align-content: start; min-width: 0; padding: 16px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  h3 { margin: 0; font-size: var(--text-13); font-weight: 600; }
  .runtime-add { display: grid; gap: 10px; }
  .field-row { display: flex; gap: 8px; min-width: 0; }
  .field-row input { flex: 1; min-width: 0; }
  .ssh-options { display: flex; flex-wrap: wrap; gap: 8px 12px; color: var(--muted); font-size: var(--text-12); }
  .ssh-options label { display: inline-flex; align-items: center; gap: 6px; min-width: 0; }
  .ssh-options input { width: 200px; min-width: 0; }
  .ssh-options .port { width: 64px; }
  .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 12px; }
  .quiet { background: transparent; color: var(--muted); }
  .quiet:hover:not(:disabled) { color: var(--ink); }
  .state, .error { margin: 0; font-size: var(--text-13); }
  .state { color: var(--muted); }
  .state.good { color: var(--ink); }
  .error { color: var(--ink); }
</style>

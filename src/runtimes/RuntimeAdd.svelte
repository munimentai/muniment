<script>
  // Adds a runtime: paste the one-time pairing link it shows, or name an SSH
  // host and the app runs the pairing command there and keeps a tunnel open
  // while it attaches. The link's secret goes to the runtime and nowhere else:
  // the field clears once pairing finishes.
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import SegmentedField from '../lib/ui/SegmentedField.svelte'
  import { DEFAULT_SSH_COMMAND, DEFAULT_SSH_PORT, errorLine, looksLikePairingLink } from './runtimes.js'

  let { tauri, deviceName = '', onpaired = () => {} } = $props()
  let mode = $state('link')
  let link = $state('')
  let name = $state('')
  let host = $state('')
  let command = $state(DEFAULT_SSH_COMMAND)
  let port = $state(String(DEFAULT_SSH_PORT))
  let pending = $state(false)
  let error = $state('')
  const id = $props.id()
  const modes = [{ value: 'link', label: 'Pairing link' }, { value: 'ssh', label: 'Over SSH' }]
  const portValid = $derived(/^\d{1,5}$/.test(port.trim()) && Number(port) >= 1 && Number(port) <= 65535)
  const ready = $derived(mode === 'link' ? looksLikePairingLink(link) : !!host.trim() && !!command.trim() && portValid)

  async function pair(event) {
    event.preventDefault()
    if (!ready || pending) return
    pending = true
    error = ''
    try {
      const deviceNameValue = name.trim() || null
      const next = mode === 'link'
        ? await tauri.invoke('runtime_pair_link', { link: link.trim(), deviceName: deviceNameValue })
        : await tauri.invoke('runtime_pair_ssh', { host: host.trim(), command: command.trim(), port: Number(port), deviceName: deviceNameValue })
      link = ''
      host = ''
      onpaired(next)
    } catch (failure) {
      error = errorLine(failure, 'Pairing did not finish. Try again.')
    } finally {
      pending = false
    }
  }
</script>

<Group heading="Add a runtime" description="Pair over your network or SSH. No account is needed.">
  <form class="runtime-add" aria-label="Add a runtime" onsubmit={pair}>
    <div class="mode"><SegmentedField label="Add by" hideLabel value={mode} options={modes} onchange={(next) => { mode = next; error = '' }} /></div>
    {#if mode === 'link'}
      <div class="field wide">
        <label for="{id}-link">Pairing link</label>
        <input id="{id}-link" class="record" type="text" autocomplete="off" spellcheck="false" placeholder="muniment://pair?…" aria-describedby="{id}-link-help" disabled={pending} bind:value={link}>
        <small id="{id}-link-help">Create the link on the runtime. It works once.</small>
      </div>
    {:else}
      <div class="field wide">
        <label for="{id}-host">SSH host</label>
        <input id="{id}-host" type="text" autocomplete="off" spellcheck="false" placeholder="build-box or me@build-box" aria-describedby="{id}-host-help" disabled={pending} bind:value={host}>
        <small id="{id}-host-help">Any host your ssh config knows. Your SSH keys sign in.</small>
      </div>
      <div class="field">
        <label for="{id}-command">Remote command</label>
        <input id="{id}-command" class="record" type="text" autocomplete="off" spellcheck="false" aria-describedby="{id}-command-help" disabled={pending} bind:value={command}>
      </div>
      <div class="field">
        <label for="{id}-port">Runtime port</label>
        <input id="{id}-port" class="record" type="text" inputmode="numeric" autocomplete="off" disabled={pending} bind:value={port}>
      </div>
      <small id="{id}-command-help" class="wide">For a container, run the command through docker exec, such as <code>docker exec muniment-runtime muniment-server</code>.</small>
    {/if}
    <div class="field wide">
      <label for="{id}-name">Name for this computer</label>
      <input id="{id}-name" type="text" autocomplete="off" maxlength="80" placeholder={deviceName} disabled={pending} bind:value={name}>
    </div>
    <div class="actions wide">
      <Button type="submit" variant="primary" disabled={!ready || pending}>{mode === 'link' ? 'Pair' : 'Pair over SSH'}</Button>
      {#if pending}<p class="state" role="status">{mode === 'link' ? 'Pairing with the runtime…' : 'Running the pairing command over SSH…'}</p>{/if}
    </div>
    {#if error}<p class="error wide" role="alert">{error}</p>{/if}
  </form>
</Group>

<style>
  .runtime-add { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 140px); gap: 12px 16px; padding: 12px 0; }
  .wide, .mode { grid-column: 1 / -1; }
  .mode { max-width: 320px; }
  .field { display: grid; gap: 6px; min-width: 0; }
  .field > label { color: var(--muted); font-size: var(--text-13); }
  code { font: var(--text-12) var(--font-mono); }
  .field input { width: 100%; min-width: 0; }
  .record { font-family: var(--font-mono); }
  small { color: var(--muted); font-size: var(--text-12); }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; }
  .state, .error { margin: 0; font-size: var(--text-13); }
  .state { color: var(--muted); }
  .error { color: var(--ink); }
</style>

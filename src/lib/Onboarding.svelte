<script>
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { onboardingCancelSettingsState, onboardingStatusState } from './onboarding-state.js'
  import { scanRows, scanSummary } from './onboarding-scan.js'
  import { firstRunError } from './onboarding-diagnostics.js'

  let { tauri, onboarding = $bindable(), draft = $bindable(''), onready } = $props()
  let panel = $state(null)
  let report = $state(null)
  let scanError = $state('')
  let modelError = $state('')
  let modelOpening = $state(false)
  let busy = $state(false)
  let picking = $state(false)
  let pickerOutcome = $state({ status: 'not-started' })
  let homeRequest = 0
  let scanRequest = 0
  const settings = $derived(!!onboarding.savedHomePath)
  let cleanupKeydown
  const rows = $derived(scanRows(report))

  async function loadHome() {
    const request = ++homeRequest
    try {
      const status = await tauri.invoke('home_status')
      if (request === homeRequest) onboarding = onboardingStatusState(status)
    } catch (_) {
      if (request === homeRequest) onboarding = { name: 'load-error', homePath: '', error: 'Muniment could not read Home. Retry or choose a folder.' }
    }
  }

  async function loadScan() {
    const request = ++scanRequest
    scanError = ''
    try {
      const result = await tauri.invoke('onboarding_scan')
      if (request === scanRequest) report = result
    } catch (_) {
      if (request === scanRequest) scanError = 'Muniment could not scan assistant memory. Retry the scan.'
    }
  }

  async function chooseHome() {
    if (busy || picking || modelOpening) return
    picking = true
    pickerOutcome = { status: 'pending' }
    const request = homeRequest
    try {
      const picked = await open({ directory: true, multiple: false, defaultPath: onboarding.homePath || undefined })
      pickerOutcome = { status: 'resolved', value: picked }
      if (request === homeRequest && typeof picked === 'string' && picked.trim()) {
        homeRequest += 1
        modelError = ''
        onboarding = { ...onboarding, name: settings ? 'settings' : 'choosing', homePath: picked, configured: false, error: undefined }
      }
    } catch (error) {
      pickerOutcome = { status: 'rejected', error: error instanceof Error ? error.message : String(error) }
      onboarding = { ...onboarding, error: 'Muniment could not open the folder picker. Try again.' }
    } finally {
      picking = false
    }
  }

  async function send() {
    if (busy || picking || (!settings && !draft.trim())) return
    if (!onboarding.homePath) {
      panel = 'home'
      onboarding = { ...onboarding, error: firstRunError(tauri, 'homeUnavailable').message }
      return
    }
    busy = true
    homeRequest += 1
    onboarding = { ...onboarding, error: undefined }
    try {
      if (!onboarding.configured) await tauri.invoke('home_confirm', { homePath: onboarding.homePath })
      onboarding = settings
        ? { name: 'complete', homePath: onboarding.homePath }
        : { ...onboarding, configured: true }
      panel = settings ? null : 'model'
    } catch (_) {
      panel = 'home'
      onboarding = { ...onboarding, error: settings
        ? 'Muniment could not save Home. Check folder access and try Save Home again.'
        : firstRunError(tauri, 'homeConfirm').message }
    } finally {
      busy = false
    }
  }

  async function openModelSettings() {
    if (busy || picking || modelOpening) return
    modelOpening = true
    modelError = ''
    try {
      await onready()
    } catch (error) {
      modelError = error instanceof Error ? error.message : String(error)
    } finally {
      modelOpening = false
    }
  }

  function focusMessage(element) {
    if (!document.querySelector('[role="dialog"]')) element.focus()
  }

  function keydown(event) {
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault()
      void send()
    }
  }

  function cancelSettings() {
    homeRequest += 1
    onboarding = onboardingCancelSettingsState(onboarding)
  }

  onMount(() => {
    // The scan probes assistant folders, some under Documents, so it runs on the
    // first run only and never raises a folder prompt on a later launch.
    void loadHome().then(() => { if (onboarding.name !== 'complete') void loadScan() })
    const leave = (event) => {
      if (event.key !== 'Escape' || !settings || busy || picking) return
      event.preventDefault()
      cancelSettings()
    }
    document.addEventListener('keydown', leave)
    cleanupKeydown = () => document.removeEventListener('keydown', leave)
    return () => { cleanupKeydown?.(); homeRequest += 1; scanRequest += 1 }
  })
</script>

{#if onboarding.name !== 'complete'}
  <section class="onboarding" aria-label={settings ? 'Home settings' : 'First run'} data-home-picker={JSON.stringify(pickerOutcome)}>
    {#if settings}
      <h1>Home settings</h1>
      <p class="path" data-testid="onboarding-home-path">{onboarding.homePath}</p>
      <div class="actions">
        <button data-testid="onboarding-picker" onclick={chooseHome} disabled={busy || picking || modelOpening}>Change folder…</button>
        <button data-testid="onboarding-cancel" onclick={cancelSettings} disabled={busy || picking}>Cancel</button>
        <button class="primary" onclick={send} disabled={busy || picking}>Save Home</button>
      </div>
    {:else}
      <div class="composer">
        <label class="visually-hidden" for="first-message">Message</label>
        <textarea id="first-message" use:focusMessage bind:value={draft} onkeydown={keydown} rows="3" placeholder="Ask anything" aria-describedby="first-message-hint"></textarea>
        <div class="composer-row">
          <span id="first-message-hint">Your memory stays on this device.</span>
          <button class="primary" onclick={send} aria-disabled={busy || picking ? 'true' : undefined}>Send</button>
        </div>
      </div>
      <div class="chips" aria-label="First-run settings">
        <button data-testid="onboarding-model" aria-expanded={panel === 'model'} aria-controls="onboarding-model-panel" onclick={() => { panel = panel === 'model' ? null : 'model' }}>Connect a model</button>
        <button class="home-chip" data-testid="onboarding-home-path" aria-expanded={panel === 'home'} aria-controls="onboarding-home-panel" onclick={() => { panel = panel === 'home' ? null : 'home' }}>{onboarding.homePath || (onboarding.name === 'loading' ? 'Finding Home…' : 'Home unavailable')}</button>
        <button data-testid="onboarding-scan" aria-expanded={panel === 'scan'} aria-controls="onboarding-scan-panel" onclick={() => { panel = panel === 'scan' ? null : 'scan' }}>{scanError ? 'Scan unavailable' : report ? (report.errors.length && !rows.some((row) => row.fileCount > 0) ? 'Assistant memory scan incomplete' : scanSummary(rows)) : 'Scanning assistant memory…'}</button>
      </div>
      {#if panel === 'model'}
        <section class="panel" id="onboarding-model-panel" aria-labelledby="model-title">
          <h2 id="model-title">Connect a model</h2>
          <p>No free hosted model exists at the no-account tier.</p>
          <p>Use your own provider key or a local server.</p>
          {#if modelError}<p class="error" role="alert">{modelError}</p>{/if}
          {#if onboarding.configured}
            <button onclick={openModelSettings} disabled={busy || picking || modelOpening}>Open model settings</button>
          {/if}
        </section>
      {:else if panel === 'home'}
        <section class="panel" id="onboarding-home-panel" aria-labelledby="home-title">
          <h2 id="home-title">Home</h2>
          <p class="path">{onboarding.homePath}</p>
          <p>The first Send creates memory/, agents/, projects/, and sessions/.</p>
          <button data-testid="onboarding-picker" onclick={chooseHome} disabled={busy || picking || modelOpening}>Change folder…</button>
          {#if !onboarding.homePath}<button onclick={loadHome} disabled={busy || picking}>Retry Home</button>{/if}
        </section>
      {:else if panel === 'scan'}
        <section class="panel" id="onboarding-scan-panel" aria-labelledby="scan-title">
          <h2 id="scan-title">Assistant memory</h2>
          {#if report}
            {#if rows.length}
              <ul aria-label="Assistant memory">
                {#each rows as row}
                  <li><span>{row.displayName}: {row.fileCount} files</span>{#if row.capped}<span class="note">Scan cap reached. The count may be incomplete.</span>{/if}{#if row.unreadable}<span class="note">Some folders could not be read.</span>{/if}</li>
                {/each}
              </ul>
            {:else if !report.errors.length}<p>No assistant memory found</p>{/if}
            {#each report.errors as error}<p class="path">{error.assistantId}: {error.message}</p>{/each}
          {/if}
          {#if scanError}<p role="alert">{scanError}</p><button onclick={loadScan}>Retry scan</button>{/if}
        </section>
      {/if}
    {/if}
    {#if onboarding.error}<p class="error" role="alert">{onboarding.error}</p>{/if}
  </section>
{/if}

<style>
  .onboarding { display: flex; flex-direction: column; width: min(760px, 100%); min-height: 0; max-height: calc(100% - 48px); align-self: start; margin-top: 48px; padding: 2px; box-sizing: border-box; }
  .composer { flex: none; padding: 16px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .composer:focus-within { border-color: var(--muted); }
  textarea { display: block; width: 100%; min-height: 90px; max-height: 35vh; resize: vertical; box-sizing: border-box; border: 0; outline: none; color: var(--ink); background: transparent; font: var(--text-15) var(--font-human); line-height: 1.55; }
  textarea::placeholder { color: var(--muted); }
  .composer-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .actions { display: flex; align-items: center; justify-content: flex-start; gap: 12px; }
  .actions .primary { margin-left: auto; }
  .composer-row { margin-top: 12px; color: var(--muted); font-size: var(--text-12); }
  .chips { flex: none; display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  .chips button { max-width: 100%; border-radius: var(--radius-chip); font: var(--text-12) var(--font-mono); text-align: left; overflow-wrap: anywhere; }
  .home-chip { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chips button[aria-expanded="true"] { border-color: var(--muted); background: var(--faint); }
  .panel { min-height: 0; overflow-y: auto; margin-top: 16px; padding: 16px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  h1 { font-size: var(--text-22); }
  h2 { margin: 0 0 12px; font-size: var(--text-15); font-weight: 600; }
  p { margin: 8px 0; font-size: var(--text-13); overflow-wrap: anywhere; }
  .path, li, .error { font: var(--text-12) var(--font-mono); }
  ul { margin: 0; padding: 0; list-style: none; }
  li { display: flex; flex-direction: column; gap: 4px; padding: 10px 0; border-top: 1px solid var(--border); overflow-wrap: anywhere; }
  .note, .error { color: var(--muted); }
  button { min-height: 28px; padding: 5px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); cursor: pointer; }
  button:hover:not(:disabled) { border-color: var(--muted); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  button:disabled, button[aria-disabled="true"] { color: var(--muted); cursor: default; }
  .primary { background: var(--ink); border-color: var(--ink); color: var(--paper); }
  .visually-hidden { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  @media (max-height: 600px) { section.onboarding { margin-top: 12px; max-height: calc(100% - 12px); } }
</style>

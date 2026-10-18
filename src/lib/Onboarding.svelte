<script>
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { onboardingCancelSettingsState, onboardingStatusState } from './onboarding-state.js'
  import { firstRunError } from './onboarding-diagnostics.js'
  import FirstRunConnect from './FirstRunConnect.svelte'
  import { scanProviders } from './first-run-providers.js'
  import { loginEvent, loginStart } from './account-login.js'

  // The first run asks one question, which model to connect, and Home settings
  // reuse this screen to change the Home folder.
  // `onready` answers `{ signIn }` with the account to sign in to when a
  // subscription signs in here, and `onsignedin` leaves the first run after it.
  let { tauri, onboarding = $bindable(), onready, onsignedin = async () => {}, listen = (...args) => window.__TAURI__?.event?.listen(...args) } = $props()
  let signIn = $state(null)
  let unlistenSignIn = null
  let modelError = $state('')
  let modelOpening = $state(false)
  let busy = $state(false)
  let picking = $state(false)
  let pickerOutcome = $state({ status: 'not-started' })
  let homeRequest = 0
  let providerScan = $state(null)
  let providerScanning = $state(false)
  let providerScanError = $state('')
  const settings = $derived(!!onboarding.savedHomePath)
  let cleanupKeydown

  async function loadHome() {
    const request = ++homeRequest
    try {
      const status = await tauri.invoke('home_status')
      if (request === homeRequest) onboarding = onboardingStatusState(status)
    } catch (_) {
      if (request === homeRequest) onboarding = { name: 'load-error', homePath: '', error: 'Muniment could not read Home. Quit and reopen Muniment.' }
    }
  }

  // The provider scan probes loopback model servers and looks for signed-in
  // assistant CLIs. It reads no file body and no credential. At first launch
  // the runtime may still be starting, so a failed scan runs again before the
  // error shows.
  let scanRequest = 0
  async function loadProviderScan() {
    const request = ++scanRequest
    providerScanning = true
    providerScanError = ''
    try {
      const scan = await scanProviders((command) => tauri.invoke(command))
      if (request === scanRequest) providerScan = scan
    } catch (_) {
      if (request === scanRequest) providerScanError = 'The scan for local models failed.'
    } finally {
      if (request === scanRequest) providerScanning = false
    }
  }

  // A choice saves Home, leaves the first run and connects the option. No
  // option opens the full provider list, and explore opens the app without one.
  async function connect(option, input = {}) {
    if (busy || picking || modelOpening) return
    if (!onboarding.homePath) {
      onboarding = { ...onboarding, error: firstRunError(tauri, 'homeUnavailable').message }
      return
    }
    busy = true
    modelError = ''
    homeRequest += 1
    try {
      if (!onboarding.configured) await tauri.invoke('home_confirm', { homePath: onboarding.homePath })
      onboarding = { ...onboarding, configured: true, error: undefined }
    } catch (_) {
      onboarding = { ...onboarding, error: firstRunError(tauri, 'homeConfirm').message }
      return
    } finally {
      busy = false
    }
    modelOpening = true
    try {
      const answer = await onready(option, input)
      if (answer?.signIn) await startSignIn(option, answer.signIn)
    } catch (error) {
      modelError = error instanceof Error ? error.message : String(error)
    } finally {
      modelOpening = false
    }
  }

  // A subscription signs in from the first run: the browser opens, and its
  // card shows the progress, a code or a question until the account connects.
  function stopSignIn() {
    const stop = unlistenSignIn
    unlistenSignIn = null
    if (stop) void Promise.resolve(stop).then((fn) => fn?.())
  }

  async function startSignIn(option, account) {
    stopSignIn()
    signIn = { ...loginStart(account.provider, option.name), option: option.id }
    unlistenSignIn = listen('local-mode-login', ({ payload }) => onSignInEvent(payload))
    try {
      await tauri.invoke('local_mode_account_login_start', { provider: account.provider })
    } catch (error) {
      stopSignIn()
      signIn = { ...signIn, stage: 'failed', message: String(error?.message ?? error) || 'The sign-in did not start. Try again.' }
    }
  }

  function onSignInEvent(payload) {
    const step = loginEvent(signIn, payload)
    if (!step) return
    signIn = { ...step.login, option: signIn.option }
    if (step.open) void tauri.invoke('local_mode_open_url', { url: step.open }).catch(() => {})
    if (step.stop) stopSignIn()
    if (step.done) void finishSignIn()
  }

  async function finishSignIn() {
    signIn = { ...signIn, message: 'Signed in. Loading its models…' }
    try {
      await onsignedin()
      signIn = null
    } catch (error) {
      signIn = { ...signIn, stage: 'failed', message: error instanceof Error ? error.message : String(error) }
    }
  }

  async function answerSignIn(value) {
    const prompt = signIn?.prompt
    if (!prompt) return
    try {
      await tauri.invoke('local_mode_account_login_answer', { id: prompt.id, value, confirmed: null })
      signIn = { ...signIn, prompt: null }
    } catch (error) {
      modelError = String(error?.message ?? error)
    }
  }

  async function cancelSignIn() {
    try { await tauri.invoke('local_mode_account_login_cancel') } catch (_) {}
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

  async function saveHome() {
    if (busy || picking) return
    busy = true
    homeRequest += 1
    onboarding = { ...onboarding, error: undefined }
    try {
      await tauri.invoke('home_confirm', { homePath: onboarding.homePath })
      onboarding = { name: 'complete', homePath: onboarding.homePath }
    } catch (_) {
      onboarding = { ...onboarding, error: 'Muniment could not save Home. Check folder access and try Save Home again.' }
    } finally {
      busy = false
    }
  }

  function cancelSettings() {
    homeRequest += 1
    onboarding = onboardingCancelSettingsState(onboarding)
  }

  onMount(() => {
    void loadHome().then(() => { if (onboarding.name !== 'complete' && !settings) void loadProviderScan() })
    const leave = (event) => {
      if (event.key !== 'Escape' || !settings || busy || picking) return
      event.preventDefault()
      cancelSettings()
    }
    document.addEventListener('keydown', leave)
    cleanupKeydown = () => document.removeEventListener('keydown', leave)
    return () => { cleanupKeydown?.(); homeRequest += 1; stopSignIn() }
  })
</script>

{#if onboarding.name !== 'complete'}
  <section class="onboarding" aria-label={settings ? 'Home settings' : 'First run'} data-home-picker={JSON.stringify(pickerOutcome)}>
    {#if settings}
      <h1>Home settings</h1>
      <p class="path" data-testid="onboarding-home-path" data-home-path={onboarding.homePath}>{onboarding.homePath}</p>
      <div class="actions">
        <button data-testid="onboarding-picker" onclick={chooseHome} disabled={busy || picking || modelOpening}>Change folder…</button>
        <button data-testid="onboarding-cancel" onclick={cancelSettings} disabled={busy || picking}>Cancel</button>
        <button class="primary" onclick={saveHome} disabled={busy || picking}>Save Home</button>
      </div>
    {:else}
      <FirstRunConnect scan={providerScan} scanning={providerScanning} scanError={providerScanError} homePath={onboarding.homePath} homeLoading={onboarding.name === 'loading'} busy={busy || modelOpening || onboarding.name === 'loading' || (!!signIn && signIn.stage !== 'failed' && signIn.stage !== 'cancelled')} {signIn} onconnect={connect} onmore={() => connect(null)} onexplore={() => connect({ kind: 'explore' })} onrescan={loadProviderScan} onsigninanswer={answerSignIn} onsignincancel={cancelSignIn} onsigninopen={(url) => tauri.invoke('local_mode_open_url', { url }).catch(() => {})} />
      {#if modelError}<p class="error" role="alert">{modelError}</p>{/if}
    {/if}
    {#if onboarding.error}<p class="error" role="alert">{onboarding.error}</p>{/if}
  </section>
{/if}

<style>
  .onboarding { display: flex; flex-direction: column; width: min(880px, 100%); min-height: 0; max-height: calc(100% - 24px); align-self: start; margin-top: 24px; padding: 2px; box-sizing: border-box; }
  .actions { display: flex; align-items: center; justify-content: flex-start; gap: 12px; }
  .actions .primary { margin-left: auto; }
  h1 { font-size: var(--text-22); }
  p { margin: 8px 0; font-size: var(--text-13); overflow-wrap: anywhere; }
  .path { font: var(--text-12) var(--font-mono); }
  .error { color: var(--muted); font: var(--text-12) var(--font-human); }
  button { min-height: 28px; padding: 5px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }
  @media (max-height: 600px) { section.onboarding { margin-top: 12px; max-height: calc(100% - 12px); } }
</style>

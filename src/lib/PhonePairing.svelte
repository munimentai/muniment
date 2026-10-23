<script>
  import { onDestroy, onMount, tick } from 'svelte'
  import PopupClose from './PopupClose.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
  import { pairingRequests } from './pairing-requests.js'
  import { dialogDismiss } from './dialog-dismiss.js'
  import { formatDateTime } from './time-format.svelte.js'
  import {
    challengeExpired,
    pairingChallengeState,
    pairingErrorState,
    pairingLoadingState,
    pairingReadyState,
    remainingLabel,
    replacementReady,
  } from './pairing-state.js'

  // `compact` keeps only the pairing dialog, for Settings → Machines, which
  // lists the paired phone as a row, opens the dialog from Add machine and
  // revokes through `revokePair`. `onpair` hears the paired phone, or null.
  let { tauri, compact = false, onpair = null } = $props()
  let pairing = $state(pairingLoadingState())
  let challenge = $state(null)
  let dialog = $state(null)
  let error = $state('')
  let revoking = $state(false)
  let revokePending = $state(false)
  let now = $state(Date.now())
  let requesting = $state(false)
  let open = $state(false)
  let ticker = 0
  let deadlineTimer = 0
  let watching = true
  let statusController
  let challengeGeneration = 0
  let finalRead = false
  let opener

  onDestroy(() => {
    watching = false
    challengeGeneration += 1
    stopStatusPoll()
    stopChallengeTimers()
    if (open) opener?.focus?.()
  })

  function stopStatusPoll() {
    statusController?.abort()
    statusController = null
  }

  async function loadStatus() {
    if (statusController || !watching || revokePending) return
    const controller = new AbortController()
    statusController = controller
    try {
      do {
        // An expired challenge gets one final read without discarding an in-flight result.
        finalRead = false
        try {
          const status = await pairingRequests.status(tauri.invoke, controller.signal)
          if (controller.signal.aborted || !watching) return
          setPairing(pairingReadyState(status))
          revoking = status.pair ? revoking : false
          if (status.pair && open) dismissChallenge()
        } catch (_) {
          if (controller.signal.aborted || !watching) return
          if (!pairing.pair) setPairing(pairingErrorState())
        }
      } while (pairing.pair || finalRead || (challenge && !challengeExpired(challenge.expiresAt)))
    } finally {
      if (statusController === controller) statusController = null
    }
  }

  // Every change of the pairing passes the paired phone out.
  function setPairing(next) {
    pairing = next
    onpair?.(next.pair ?? null)
  }

  export async function requestChallenge() {
    if (requesting || !watching) return
    requesting = true
    const generation = ++challengeGeneration
    if (!open) {
      opener = document.activeElement
      open = true
      await tick()
      dialog?.querySelector('.popup-close')?.focus()
    }
    if (!watching || generation !== challengeGeneration) return
    error = ''
    try {
      const response = await pairingRequests.challenge(tauri.invoke)
      if (!watching || generation !== challengeGeneration) return
      challenge = pairingChallengeState(response)
      startClock()
      void loadStatus()
    } catch (err) {
      if (!watching || generation !== challengeGeneration) return
      const message = typeof err === 'string' ? err : err?.message
      error = message === 'A phone is already paired.' || message === 'Wait ten seconds before replacing the code.'
        ? message
        : 'The pairing request failed.'
      if (message === 'A phone is already paired.') void loadStatus()
    } finally {
      if (watching && generation === challengeGeneration) requesting = false
    }
  }

  function checkDeadline() {
    now = Date.now()
    if (challenge && challengeExpired(challenge.expiresAt, now) && deadlineTimer) {
      clearTimeout(deadlineTimer)
      deadlineTimer = 0
      finalRead = true
      void loadStatus()
    }
  }

  function startClock() {
    stopChallengeTimers()
    now = Date.now()
    deadlineTimer = setTimeout(checkDeadline, Math.max(0, Date.parse(challenge.expiresAt) - now))
    ticker = setInterval(checkDeadline, 1000)
  }

  function stopChallengeTimers() {
    clearInterval(ticker)
    clearTimeout(deadlineTimer)
    ticker = 0
    deadlineTimer = 0
  }

  function dismissChallenge() {
    open = false
    challenge = null
    finalRead = false
    challengeGeneration += 1
    requesting = false
    stopChallengeTimers()
    void tick().then(() => { if (watching) opener?.focus?.() })
  }

  function closeChallenge() {
    dismissChallenge()
    stopStatusPoll()
  }

  export async function revokePair(pairId) {
    if (revokePending) return false
    revokePending = true
    stopStatusPoll()
    error = ''
    try {
      await tauri.invoke('auth_pairing_revoke', { pairId })
      if (!watching) return
      revoking = false
      setPairing(pairingReadyState({ pair: null }))
      return true
    } catch (_) {
      if (watching) error = 'The phone could not be revoked.'
      if (compact) throw new Error('The phone could not be revoked.')
      return false
    } finally {
      revokePending = false
      if (watching && pairing.pair) void loadStatus()
    }
  }

  // A pair whose relay keys this desktop lacks is revoked and replaced by a new code.
  async function pairAgain(pairId) {
    await revokePair(pairId)
    if (watching && !pairing.pair) await requestChallenge()
  }

  function lastActive(value) {
    return formatDateTime(value)
  }

  onMount(() => {
    loadStatus()
    const keydown = (event) => {
      if (!open || !dialog) return
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopImmediatePropagation()
        closeChallenge()
      }
      if (event.key !== 'Tab') return
      const controls = [...dialog.querySelectorAll('button:not(:disabled)')]
      const first = controls[0]
      const last = controls.at(-1)
      if (!dialog.contains(document.activeElement) || (event.shiftKey ? document.activeElement === first : document.activeElement === last)) {
        event.preventDefault()
        const target = event.shiftKey ? last : first
        target?.focus()
      }
    }
    document.addEventListener('keydown', keydown, true)
    window.addEventListener('focus', checkDeadline)
    document.addEventListener('visibilitychange', checkDeadline)
    return () => {
      document.removeEventListener('keydown', keydown, true)
      window.removeEventListener('focus', checkDeadline)
      document.removeEventListener('visibilitychange', checkDeadline)
    }
  })
</script>

{#if !compact}
<section class="pairing-section" aria-labelledby="pairing-heading">
  <h3 id="pairing-heading" class="settings-heading">Phone pairing</h3>
  <div class="pairing-card">
  {#if pairing.name === 'loading'}
    <p class="access-status" aria-live="polite">Loading phone pairing…</p>
  {:else if pairing.name === 'error'}
    <div role="alert"><Row description="Phone pairing could not be loaded."><Button onclick={loadStatus}>Try again</Button></Row></div>
  {:else if pairing.pair}
    {#if revoking}
      <div class="pairing-confirm" role="group" aria-label="Revoke this phone?">
        <p>This phone loses remote control of this desktop.</p>
        {#if error}<p class="revoke-error" role="alert">{error}</p>{/if}
        <div class="pairing-actions">
          <button type="button" disabled={revokePending} onclick={() => revokePair(pairing.pair.pair_id)}>{error ? 'Retry revoke' : 'Revoke'}</button>
          <button type="button" disabled={revokePending} onclick={() => { if (!revokePending) { revoking = false; error = '' } }}>Cancel</button>
        </div>
      </div>
    {:else}
      <Row title="Paired phone" meta={pairing.pair.mobile_device_id} description={pairing.pair.remote_control === 'pair_again' ? 'This phone cannot open sessions from this desktop.' : ''}>
        {#snippet detail()}<time datetime={pairing.pair.created_at}>Paired {lastActive(pairing.pair.created_at)}</time>{/snippet}
        {#if pairing.pair.remote_control === 'pair_again'}<Button disabled={revokePending} onclick={() => pairAgain(pairing.pair.pair_id)}>Pair again</Button>{/if}
        <Button onclick={() => { revoking = true; error = '' }}>Revoke phone</Button>
      </Row>
    {/if}
  {:else}
    <Row title="Phone" description="No phone is paired.">
      <Button variant="primary" disabled={requesting} onclick={requestChallenge}>Pair a phone</Button>
    </Row>
    {#if error && !open}<p class="revoke-error" role="alert">{error}</p>{/if}
  {/if}
  </div>
</section>
{/if}

{#if open}
  <div class="dialog-backdrop" use:dialogDismiss={{onclose: closeChallenge}}>
    <div data-panel="confirm" data-panel-variant="overlay" class="dialog-panel" role="dialog" aria-modal="true" aria-labelledby="pairing-dialog-title" bind:this={dialog}>
      <header>
        <h2 id="pairing-dialog-title">Pair a phone</h2>
        <PopupClose label="Close pairing" onclick={closeChallenge} />
      </header>
      <p>Scan this code in the Muniment app.</p>
      {#if error}<p class="revoke-error" role="alert">{error}</p>{/if}
      {#if requesting}
        <p role="status">The pairing request is in progress.</p>
      {:else if !challenge}
        <button type="button" data-variant="primary" onclick={requestChallenge}>Try again</button>
      {:else if challengeExpired(challenge.expiresAt, now)}
        <p class="access-status" role="status">The code expired.</p>
        <button type="button" data-variant="primary" disabled={!replacementReady(challenge.requestedAt, now)} onclick={requestChallenge}>New code</button>
      {:else}
        <div class="pairing-qr">{@html challenge.qrSvg}</div>
        <p class="expiry">Code expires in {remainingLabel(challenge.expiresAt, now)}</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* A heading, then the paired phone or Pair a phone in one card row, as on every Settings page. */
  .pairing-section { display: grid; gap: 10px; }
  .pairing-section > h3 { margin: 0; }
  .pairing-card { display: grid; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .access-status { margin: 0; padding: 12px 0; color: var(--muted); font-size: var(--text-13); }
  time, .expiry { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .pairing-confirm { padding: 12px 0; }
  .pairing-confirm { color: var(--ink); font-size: var(--text-12); }
  .pairing-confirm p { margin: 0; }
  .pairing-actions { display: flex; justify-content: flex-end; gap: 5px; margin-top: 6px; }
  .pairing-actions button { padding: 3px 6px; border-color: transparent; background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); }
  .pairing-actions button:hover:not(:disabled) { background: var(--faint); }
  .revoke-error { margin: 0; padding: 0 0 12px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; min-height: 24px; min-width: 24px; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .dialog-backdrop { position: fixed; inset: 0; z-index: 11; display: grid; place-items: center; padding: 24px; background: var(--overlay-backdrop); }
  .dialog-panel { width: min(440px, 100%); padding: 24px; font-family: var(--font-human); }
  header { display: flex; align-items: start; justify-content: space-between; gap: 16px; }
  h2 { margin: 0; font-size: var(--text-22); line-height: var(--leading-heading); letter-spacing: var(--tracking-heading); }
  .dialog-panel p { margin: 12px 0 0; color: var(--muted); font-size: var(--text-15); }
  .pairing-qr { margin-top: 16px; width: min(220px, 100%); color: var(--ink); }
  .pairing-qr :global(svg) { display: block; width: 100%; height: auto; }
  .expiry { margin-top: 12px !important; font: var(--text-12) var(--font-mono) !important; }
</style>

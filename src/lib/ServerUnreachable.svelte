<script>
  import { onMount } from 'svelte'

  import { diagnosticsText, secondsUntilRetry } from './connectivity.js'

  let { state: connectivity, version, issuerHost, onretry } = $props()
  let now = $state(Date.now())
  let retrying = $state(false)
  let copied = $state(false)
  let lastDeadline = $state(0)
  let seconds = $derived(secondsUntilRetry(connectivity, now))

  onMount(() => {
    const timer = window.setInterval(() => {
      now = Date.now()
      if (connectivity.nextRetryAt !== lastDeadline) {
        lastDeadline = connectivity.nextRetryAt
        retrying = false
      }
      if (seconds === 0 && !retrying) retry()
    }, 250)
    return () => window.clearInterval(timer)
  })

  function retry() {
    if (retrying) return
    retrying = true
    onretry()
  }

  async function copyDiagnostics() {
    const text = diagnosticsText({
      version,
      timestamp: Date.now(),
      action: connectivity.action,
      issuerHost,
      message: connectivity.message,
    })
    await navigator.clipboard.writeText(text)
    copied = true
  }
</script>

<section class="unreachable" aria-labelledby="connection-title" aria-live="assertive">
  <div class="ledger">
    <p class="eyebrow">control plane · unreachable</p>
    <h1 id="connection-title">Muniment can’t reach the server.</h1>
    <p class="cause">{connectivity.message}</p>
    <dl>
      <div>
        <dt>issuer</dt>
        <dd>{issuerHost}</dd>
      </div>
      <div>
        <dt>action</dt>
        <dd>{connectivity.action}</dd>
      </div>
      <div>
        <dt>next step</dt>
        <dd>{retrying ? 'retrying now' : `retrying in ${seconds}s`}</dd>
      </div>
    </dl>
    <div class="actions">
      <button onclick={retry} disabled={retrying}>Retry now</button>
      <button onclick={copyDiagnostics}>{copied ? 'Diagnostics copied' : 'Copy diagnostics'}</button>
    </div>
  </div>
</section>

<style>
  .unreachable {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    padding: 32px;
    color: var(--ink);
    background: var(--paper);
    font-family: var(--font-mono);
  }

  .ledger {
    width: min(100%, 560px);
    border-top: 1px solid var(--ink);
    border-bottom: 1px solid var(--border);
    padding: 20px 0 24px;
  }

  .eyebrow {
    color: var(--muted);
    font-size: var(--text-12);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  h1 {
    margin-top: 28px;
    font: inherit;
    font-size: var(--text-22);
    line-height: var(--leading-heading);
  }

  .cause {
    margin-top: 10px;
    color: var(--muted);
    font-size: var(--text-13);
    overflow-wrap: anywhere;
  }

  dl {
    margin-top: 32px;
    border-top: 1px solid var(--border);
  }

  dl div {
    display: grid;
    grid-template-columns: 112px 1fr;
    gap: 16px;
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
    font-size: var(--text-12);
  }

  dt { color: var(--muted); }
  dd { overflow-wrap: anywhere; }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 24px;
  }

  button {
    min-height: 32px;
    padding: 5px 12px;
    color: var(--ink);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    font: inherit;
    font-size: var(--text-12);
    cursor: pointer;
  }

  button:hover:not(:disabled) { border-color: var(--muted); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  button:disabled { color: var(--muted); cursor: default; }

  @media (max-width: 480px) {
    .unreachable { padding: 20px; }
    dl div { grid-template-columns: 88px 1fr; }
  }
</style>

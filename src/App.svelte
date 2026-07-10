<script>
  import { ringPath } from './lib/mark.js'

  const markD = ringPath()
  const version = __APP_VERSION__

  // Temporary trigger for the Rust auth commands (Phase 2.8b) — the real
  // signed-in UI is the next slice. Hidden when the page is served outside
  // Tauri (plain `npm run dev` in a browser).
  const tauri = window.__TAURI__?.core
  let authLine = $state('')
  let busy = $state(false)

  async function call(command) {
    busy = true
    authLine = command === 'auth_sign_in' ? 'waiting for browser sign-in…' : '…'
    try {
      const status = await tauri.invoke(command)
      authLine = status.signed_in
        ? `signed in as ${status.subject ?? 'unknown subject'}`
        : 'signed out'
    } catch (err) {
      authLine = `error: ${err}`
    } finally {
      busy = false
    }
  }
</script>

<main>
  <div class="lockup">
    <svg width="34" height="34" viewBox="0 0 48 48" role="img" aria-label="muniment">
      <path d={markD} stroke-width="4.5" />
    </svg>
    <span class="name">muniment</span>
  </div>
  <p class="meta">shell v{version}</p>

  {#if tauri}
    <div class="auth-dev">
      <button onclick={() => call('auth_sign_in')} disabled={busy}>sign in</button>
      <button onclick={() => call('auth_status')} disabled={busy}>status</button>
      <button onclick={() => call('auth_sign_out')} disabled={busy}>sign out</button>
    </div>
    {#if authLine}
      <p class="auth-line">{authLine}</p>
    {/if}
  {/if}
</main>

<style>
  main {
    min-height: 100vh;
    display: grid;
    place-content: center;
    justify-items: center;
  }

  /* Lockup (§1.8): mark at rest — static, ink — beside the wordmark,
     Schibsted 600, lowercase, −1% tracking. */
  .lockup {
    display: flex;
    align-items: center;
    gap: 13px;
  }

  .lockup path {
    fill: none;
    stroke: var(--ink);
    stroke-linecap: round;
  }

  .name {
    font-size: 30px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .meta {
    margin-top: 18px;
    font-family: var(--font-mono);
    font-size: var(--text-12);
    color: var(--muted);
  }

  /* Temporary auth trigger (Phase 2.8b) — replaced by the signed-in UI. */
  .auth-dev {
    margin-top: 34px;
    display: flex;
    gap: 8px;
  }

  .auth-dev button {
    font: inherit;
    font-size: var(--text-13);
    color: var(--ink);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    padding: 5px 12px;
    cursor: pointer;
  }

  .auth-dev button:hover:not(:disabled) {
    border-color: var(--muted);
  }

  .auth-dev button:focus-visible {
    outline: 2px solid var(--signal);
    outline-offset: 1px;
  }

  .auth-dev button:disabled {
    color: var(--muted);
    cursor: default;
  }

  .auth-line {
    margin-top: 12px;
    font-family: var(--font-mono);
    font-size: var(--text-12);
    color: var(--muted);
  }
</style>

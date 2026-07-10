<script>
  import { onMount } from 'svelte'

  import { bootState, errorKind, errorMessage, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { connectedState, enterUnreachable, resetConnectivity } from './lib/connectivity.js'
  import { ringPath } from './lib/mark.js'
  import ServerUnreachable from './lib/ServerUnreachable.svelte'

  const markD = ringPath()
  const version = __APP_VERSION__

  const tauri = window.__TAURI__?.core
  let auth = $state(bootState)
  let connectivity = $state(connectedState)
  let issuerHost = $state('api.muniment.ai')

  async function run(action) {
    const command = {
      status: 'auth_status',
      'sign-in': 'auth_sign_in',
      'sign-out': 'auth_sign_out',
      'ensure-fresh': 'auth_ensure_fresh',
    }[action]

    if (action === 'sign-in') auth = waitingState()
    try {
      const status = await tauri.invoke(command)
      connectivity = resetConnectivity()
      auth = statusState(status)
    } catch (err) {
      if (errorKind(err) === 'network' && action !== 'sign-out') {
        connectivity = enterUnreachable(
          connectivity,
          { action, message: errorMessage(err) },
          Date.now(),
        )
        return
      }
      auth = errorState(action, err)
    }
  }

  onMount(() => {
    if (tauri) {
      tauri.invoke('auth_issuer_host').then((host) => { issuerHost = host })
      run('status')
    }
  })
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
    {#if auth.name === 'signed-out'}
      <section class="auth-state">
        <p class="support">Sign in to continue to your workspace.</p>
        <button onclick={() => run('sign-in')}>Sign in</button>
      </section>
    {:else if auth.name === 'signing-in'}
      <section class="auth-state" aria-live="polite">
        <button disabled>Sign in</button>
        <p class="record">Waiting for the browser sign-in…</p>
      </section>
    {:else if auth.name === 'signed-in'}
      <section class="auth-state profile">
        <p class="subject">{auth.subject}</p>
        <p class="record">signed in · local session</p>
        <button onclick={() => run('sign-out')}>Sign out</button>
      </section>
    {:else if auth.name === 'error'}
      <section class="auth-state" aria-live="polite">
        <p class="record error-record">{auth.message}</p>
        <button onclick={() => run(auth.retry)}>Try again</button>
      </section>
    {/if}
  {/if}
</main>

{#if connectivity.name === 'unreachable'}
  <ServerUnreachable
    state={connectivity}
    {version}
    {issuerHost}
    onretry={() => run(connectivity.action)}
  />
{/if}

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

  .auth-state {
    margin-top: 34px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    max-width: 420px;
    text-align: center;
  }

  button {
    font: inherit;
    font-size: var(--text-13);
    color: var(--ink);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    padding: 5px 12px;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    border-color: var(--muted);
  }

  button:focus-visible {
    outline: 2px solid var(--signal);
    outline-offset: 1px;
  }

  button:disabled {
    color: var(--muted);
    cursor: default;
  }

  .support {
    color: var(--muted);
  }

  .subject {
    font-size: var(--text-17);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .record {
    font-family: var(--font-mono);
    font-size: var(--text-12);
    color: var(--muted);
  }

  .profile button {
    margin-top: 8px;
  }

  .error-record {
    line-height: var(--leading-body);
  }
</style>

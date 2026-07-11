<script>
  import { onMount } from 'svelte'

  import { bootState, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { ringPath } from './lib/mark.js'
  import { applyChatEvent, receiptParts, shouldSend } from './lib/chat-state.js'

  const markD = ringPath()
  const version = __APP_VERSION__

  const tauri = window.__TAURI__?.core
  let auth = $state(bootState)
  let draft = $state('')
  let messages = $state([])
  let active = $state(null)
  let buffered = new Map()
  let unlisten

  async function run(action) {
    const command = {
      status: 'auth_status',
      'sign-in': 'auth_sign_in',
      'sign-out': 'auth_sign_out',
    }[action]

    if (action === 'sign-in') auth = waitingState()
    try {
      const status = await tauri.invoke(command)
      auth = statusState(status)
    } catch (err) {
      auth = errorState(action, err)
    }
  }

  onMount(() => {
    if (tauri) run('status')
    window.__TAURI__?.event?.listen('chat-event', ({ payload }) => {
      if (!messages.some((message) => message.run?.id === payload.runId)) {
        buffered.set(payload.runId, payload)
        return
      }
      const projected = applyChatEvent(active, payload)
      if (projected) messages = messages.map((message) => message.run?.id === projected.id ? { ...message, run: projected } : message)
      active = projected && !['complete', 'cancelled', 'failed'].includes(projected.phase) ? projected : null
    }).then((stop) => { unlisten = stop })
    return () => unlisten?.()
  })

  async function send() {
    const prompt = draft.trim()
    if (!prompt || active) return
    draft = ''
    messages.push({ role: 'user', text: prompt })
    const pending = { id: 'pending', phase: 'thinking', text: '', receipt: null, prompt }
    active = pending
    messages.push({ role: 'assistant', run: pending })
    try {
      const run = await tauri.invoke('chat_submit', { prompt })
      active = { ...pending, id: run.runId }
      const early = buffered.get(run.runId)
      if (early) { active = applyChatEvent(active, early); buffered.delete(run.runId) }
      messages = messages.map((message) => message.run === pending ? { ...message, run: active } : message)
    } catch (_) {
      const failed = { ...pending, id: `rejected-${messages.length}`, phase: 'failed' }
      messages = messages.map((message) => message.run === pending ? { ...message, run: failed } : message)
      active = null
    }
  }

  function keydown(event) {
    if (shouldSend(event, draft, Boolean(active))) {
      event.preventDefault()
      send()
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
      <section class="workspace">
        <header><span>New thread</span><button class="quiet" onclick={() => run('sign-out')}>Sign out</button></header>
        <div class="thread" aria-live="polite">
          {#if messages.length === 0}<p class="empty">Ask anything. Your org's routing decides which model answers.</p>{/if}
          {#each messages as message}
            {#if message.role === 'user'}<p class="user-message">{message.text}</p>
            {:else}<div class="response">
              {#if message.run.phase === 'thinking'}
                <span class="thinking"><svg width="17" height="17" viewBox="0 0 48 48" aria-label="Thinking"><path d={markD} stroke-width="5" /></svg><span>Routing</span></span>
              {:else}<p class:streaming={message.run.phase === 'streaming'}>{message.run.text}{#if message.run.phase === 'streaming'}<span class="caret" aria-hidden="true"></span>{/if}</p>{/if}
              {#if message.run.phase === 'failed'}<div class="run-error">Reply failed. <button onclick={() => { draft = message.run.prompt; send() }}>Try again</button></div>{/if}
              {#if message.run.phase === 'complete'}{@const parts = receiptParts(message.run.receipt)}{#if parts.length}<button class="provenance" aria-label={parts.join(', ')}><span>{parts[0]}</span>{#if parts.length > 1} · {parts.slice(1).join(' · ')}{/if}</button>{/if}{/if}
            </div>{/if}
          {/each}
        </div>
        <div class="composer">
          <textarea bind:value={draft} onkeydown={keydown} rows="2" placeholder="Ask anything" disabled={Boolean(active)}></textarea>
          <div class="composer-row"><span>Routing is automatic. Every reply carries its receipt.</span>{#if active}<button onclick={() => tauri.invoke('chat_cancel', { runId: active.id })}>Stop</button>{:else}<button disabled={!draft.trim()} onclick={send}>Send</button>{/if}</div>
        </div>
      </section>
    {:else if auth.name === 'error'}
      <section class="auth-state" aria-live="polite">
        <p class="record error-record">{auth.message}</p>
        <button onclick={() => run(auth.retry)}>Try again</button>
      </section>
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

  .record {
    font-family: var(--font-mono);
    font-size: var(--text-12);
    color: var(--muted);
  }

  .error-record {
    line-height: var(--leading-body);
  }

  .workspace { position: fixed; inset: 0; display: grid; grid-template-rows: 52px 1fr auto; }
  .workspace header { display: flex; justify-content: space-between; align-items: center; padding: 0 24px; border-bottom: 1px solid var(--border); font-weight: 600; }
  .quiet { background: transparent; border-color: transparent; }
  .thread { width: min(760px, calc(100% - 48px)); margin: 0 auto; padding: 42px 0; overflow-y: auto; }
  .empty { color: var(--muted); text-align: center; margin-top: 18vh; }
  .user-message { width: fit-content; max-width: 78%; margin: 0 0 28px auto; padding: 9px 13px; white-space: pre-wrap; background: var(--faint); border-radius: 10px; }
  .response { margin: 0 0 34px; }
  .response p { white-space: pre-wrap; }
  .streaming { display: inline; border-bottom: 2px solid var(--signal); }
  .caret { display: inline-block; height: 1em; border-right: 2px solid var(--signal); margin-left: 2px; vertical-align: -2px; animation: blink 800ms step-end infinite; }
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thinking path { fill: none; stroke: var(--signal); stroke-linecap: round; animation: breathe 1.8s ease-in-out infinite; }
  .provenance { display: block; margin-top: 10px; padding: 0; border: 0; background: transparent; color: var(--muted); font: var(--text-12) var(--font-mono); text-align: left; }
  .provenance span { color: var(--signal); }
  .run-error { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .run-error button { padding: 2px 6px; }
  .composer { width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: 10px; }
  .composer:focus-within { border-color: var(--muted); }
  textarea { width: 100%; resize: none; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; }
  .composer-row { display: flex; justify-content: space-between; align-items: center; color: var(--muted); font-size: 11px; }
  @keyframes blink { 50% { opacity: 0; } }
  @keyframes breathe { 50% { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) { .caret, .thinking path { animation: none; } }
</style>

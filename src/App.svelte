<script>
  import { onMount, tick } from 'svelte'

  import { bootState, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { ringPath } from './lib/mark.js'
  import { applyBufferedChatEvents, applyChatEvent, composerAction, receiptParts, receiptRows } from './lib/chat-state.js'
  import { scrollFollowState } from './lib/scroll-follow.js'

  const markD = ringPath()
  const version = __APP_VERSION__

  const tauri = window.__TAURI__?.core
  let auth = $state(bootState)
  let draft = $state('')
  let messages = $state([])
  let active = $state(null)
  let cancelError = $state('')
  let queueError = $state('')
  let historyError = $state('')
  let buffered = new Map()
  let unlisten
  let thread = $state()
  let pinned = $state(true)
  let hasContentBelow = $state(false)
  let lastScrollTop = 0
  let expandedReceipts = $state(new Set())

  function toggleReceipt(runId) {
    const next = new Set(expandedReceipts)
    next.has(runId) ? next.delete(runId) : next.add(runId)
    expandedReceipts = next
  }

  function scrollToLatest() {
    if (!thread) return
    pinned = true
    const behavior = window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth'
    thread.scrollTo({ top: thread.scrollHeight, behavior })
    lastScrollTop = thread.scrollHeight - thread.clientHeight
    hasContentBelow = false
  }

  function followNewContent() {
    if (!pinned) return
    tick().then(() => {
      if (!pinned || !thread) return
      thread.scrollTo({ top: thread.scrollHeight, behavior: 'auto' })
      lastScrollTop = thread.scrollHeight - thread.clientHeight
      hasContentBelow = false
    })
  }

  function handleThreadScroll() {
    const next = scrollFollowState({
      pinned,
      scrollTop: thread.scrollTop,
      scrollHeight: thread.scrollHeight,
      clientHeight: thread.clientHeight,
      lastScrollTop,
    })
    pinned = next.pinned
    lastScrollTop = next.lastScrollTop
    hasContentBelow = !pinned && thread.scrollHeight - thread.clientHeight - thread.scrollTop > 0
  }

  $effect(() => {
    messages
    followNewContent()
  })

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
      if (auth.name === 'signed-in') await loadHistory()
    } catch (err) {
      auth = errorState(action, err)
    }
  }

  async function loadHistory() {
    historyError = ''
    expandedReceipts = new Set()
    try {
      const history = await tauri.invoke('chat_history')
      messages = history.flatMap((entry) => [
        ...(entry.prompt ? [{ role: 'user', text: entry.prompt }] : []),
        { role: 'assistant', run: { id: entry.runId, phase: entry.phase, text: entry.text, receipt: entry.receipt ?? null, prompt: entry.prompt ?? '' } },
      ])
      pinned = true
      followNewContent()
    } catch (_) {
      historyError = 'Conversation history could not be restored. Try again.'
    }
  }

  onMount(() => {
    if (tauri) run('status')
    window.__TAURI__?.event?.listen('chat-event', ({ payload }) => {
      if (!messages.some((message) => message.run?.id === payload.runId)) {
        buffered.set(payload.runId, [...(buffered.get(payload.runId) ?? []), payload])
        return
      }
      const projected = applyChatEvent(active, payload)
      if (projected) messages = messages.map((message) => message.run?.id === projected.id ? { ...message, run: projected } : message)
      active = projected && !['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? projected : null
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
    followNewContent()
    try {
      const run = await tauri.invoke('chat_submit', { prompt })
      active = { ...pending, id: run.runId }
      const early = buffered.get(run.runId) ?? []
      const projected = applyBufferedChatEvents(active, early)
      buffered.delete(run.runId)
      messages = messages.map((message) => message.run === pending ? { ...message, run: projected } : message)
      active = ['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? null : projected
    } catch (_) {
      const failed = { ...pending, id: `rejected-${messages.length}`, phase: 'failed' }
      messages = messages.map((message) => message.run === pending ? { ...message, run: failed } : message)
      active = null
    }
  }

  async function cancel() {
    cancelError = ''
    try {
      await tauri.invoke('chat_cancel', { runId: active.id })
    } catch (_) {
      cancelError = 'Could not stop this reply. Try again.'
    }
  }

  async function queue(delivery) {
    const message = draft.trim()
    if (!message || !active || active.id === 'pending') return
    const runId = active.id
    queueError = ''
    try {
      await tauri.invoke('chat_queue', { runId, delivery, message })
      messages.push({ role: 'user', text: message })
      followNewContent()
      if (draft.trim() === message) draft = ''
    } catch (err) {
      queueError = typeof err === 'string' ? err : String(err)
    }
  }

  function keydown(event) {
    const action = composerAction(event, draft, active)
    if (action) {
      event.preventDefault()
      action === 'submit' ? send() : queue('steer')
    }
  }
</script>

<main class:signed-frame={auth.name === 'signed-in'}>
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
        <header class="titlebar"><span class="thread-title">New thread</span><span class="thread-id">local · durable</span><span class="title-spacer"></span><button class="quiet" aria-label="Open artifact rail">⌘J</button></header>
        <aside class="sidebar">
          <div class="side-brand"><svg width="24" height="24" viewBox="0 0 48 48" aria-hidden="true"><path d={markD} stroke-width="5" /></svg><strong>muniment</strong></div>
          <button class="side-action">＋ <span>New thread</span><kbd>⌘N</kbd></button>
          <button class="side-action">⌕ <span>Search</span><kbd>⌘F</kbd></button>
          <p class="side-label">Threads</p>
          <button class="thread-row active-thread"><span></span>New thread</button>
          <div class="profile-block">
            <button class="profile-button"><span class="profile-initial">{auth.subject?.slice(0, 1)?.toLowerCase() ?? 'm'}</span><span><strong>{auth.subject ?? 'Signed in'}</strong><small>personal · member</small></span></button>
            <button class="quiet sign-out" onclick={() => run('sign-out')}>Sign out</button>
          </div>
        </aside>
        <div class="thread-shell">
        <div class="thread" aria-live="polite" bind:this={thread} onscroll={handleThreadScroll}>
          {#if historyError}<p class="history-error" role="alert">{historyError} <button onclick={loadHistory}>Try again</button></p>{/if}
          {#if messages.length === 0}<p class="empty">Ask anything. Your org's routing decides which model answers.</p>{/if}
          {#each messages as message}
            {#if message.role === 'user'}<p class="user-message">{message.text}</p>
            {:else}<div class="response">
              {#if message.run.phase === 'thinking'}
                <span class="thinking"><svg width="17" height="17" viewBox="0 0 48 48" aria-label="Thinking"><path d={markD} stroke-width="5" /></svg><span>Routing</span></span>
              {:else}<p class:streaming={message.run.phase === 'streaming'}>{message.run.text}{#if message.run.phase === 'streaming'}<span class="caret" aria-hidden="true"></span>{/if}</p>{/if}
              {#if message.run.phase === 'failed'}<div class="run-error">Reply failed. <button onclick={() => { draft = message.run.prompt; send() }}>Try again</button></div>{/if}
              {#if message.run.phase === 'interrupted'}<div class="run-error">Reply interrupted. {#if message.run.prompt}<button onclick={() => { draft = message.run.prompt; send() }}>Try again</button>{/if}</div>{/if}
              {#if message.run.phase === 'complete'}
                {@const parts = receiptParts(message.run.receipt)}
                {@const rows = receiptRows(message.run.receipt)}
                {#if parts.length}
                  {@const expanded = expandedReceipts.has(message.run.id)}
                  <button class="provenance" aria-expanded={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} receipt: ${parts.join(', ')}`} onclick={() => toggleReceipt(message.run.id)}><span>{parts[0]}</span>{#if parts.length > 1} · {parts.slice(1).join(' · ')}{/if}</button>
                  {#if expanded}
                    <dl class="receipt-record">
                      {#each rows as row}
                        <div><dt>{row.label}</dt><dd class:route-value={row.route}>{row.value}</dd></div>
                      {/each}
                    </dl>
                  {/if}
                {/if}
              {/if}
            </div>{/if}
          {/each}
        </div>
        {#if !pinned && hasContentBelow}<button class="latest" onclick={scrollToLatest}>↓ latest</button>{/if}
        </div>
        <div class="composer">
          <textarea bind:value={draft} onkeydown={keydown} rows="2" placeholder="Ask anything"></textarea>
          {#if cancelError}<p class="cancel-error" role="alert">{cancelError}</p>{/if}
          {#if queueError}<p class="cancel-error" role="alert">{queueError}</p>{/if}
          <div class="composer-row">
            <span>{active && active.id !== 'pending' ? '⏎ steers this reply · queue as follow-up' : 'Routing is automatic. Every reply carries its receipt.'}</span>
            <div class="composer-actions">
              {#if active && active.id !== 'pending'}
                <button class="quiet follow-up" disabled={!draft.trim()} onclick={() => queue('followUp')}>Queue follow-up</button>
                <button onclick={cancel}>Stop</button>
                <button disabled={!draft.trim()} onclick={() => queue('steer')}>Send</button>
              {:else if !active}<button disabled={!draft.trim()} onclick={send}>Send</button>{/if}
            </div>
          </div>
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
  .workspace { grid-template-columns: 260px 1fr; grid-template-areas: "title title" "side thread" "side composer"; }
  .titlebar { grid-area: title; display: flex; align-items: center; padding: 0 18px 0 278px; border-bottom: 1px solid var(--border); background: var(--surface); }
  .thread-title { font-weight: 600; }
  .thread-id, kbd { margin-left: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .title-spacer { flex: 1; }
  .sidebar { grid-area: side; min-width: 0; display: flex; flex-direction: column; padding: 14px 10px 10px; background: var(--surface); border-right: 1px solid var(--border); }
  .side-brand { display: flex; align-items: center; gap: 10px; padding: 2px 8px 16px; }
  .side-brand path { fill: none; stroke: var(--ink); stroke-linecap: round; }
  .side-action, .thread-row, .profile-button { width: 100%; display: flex; align-items: center; gap: 9px; padding: 7px 8px; border-color: transparent; background: transparent; text-align: left; }
  .side-action span { flex: 1; }
  .side-label { margin: 20px 8px 5px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .active-thread { background: var(--faint); }
  .active-thread > span { width: 5px; height: 5px; border-radius: 50%; background: var(--signal); }
  .profile-block { margin-top: auto; padding-top: 10px; border-top: 1px solid var(--border); }
  .profile-button > span:last-child { min-width: 0; display: grid; }
  .profile-button strong { overflow: hidden; text-overflow: ellipsis; font-size: var(--text-13); }
  .profile-button small { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .profile-initial { display: grid; place-items: center; width: 27px; height: 27px; border-radius: 50%; background: var(--faint); }
  .sign-out { margin: 3px 8px 0; padding-left: 0; color: var(--muted); }
  .quiet { background: transparent; border-color: transparent; }
  .thread-shell { grid-area: thread; position: relative; min-height: 0; }
  .thread { width: min(760px, calc(100% - 48px)); height: 100%; margin: 0 auto; padding: 42px 0; overflow-y: auto; }
  .latest { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%); border-radius: 6px; background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); box-shadow: 0 1px 3px color-mix(in srgb, var(--ink) 10%, transparent); }
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
  .receipt-record { width: fit-content; min-width: 240px; margin: 8px 0 0; padding: 8px 12px; border: 1px solid var(--border); border-radius: 6px; color: var(--muted); font-size: var(--text-12); }
  .receipt-record div { display: grid; grid-template-columns: 88px minmax(0, 1fr); gap: 12px; }
  .receipt-record dd { margin: 0; font-family: var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .receipt-record .route-value { color: var(--signal); }
  .run-error { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .cancel-error, .history-error { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .run-error button { padding: 2px 6px; }
  .composer { grid-area: composer; width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: 10px; }
  .composer:focus-within { border-color: var(--muted); }
  textarea { width: 100%; resize: none; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; }
  .composer-row { display: flex; justify-content: space-between; align-items: center; color: var(--muted); font-size: 11px; }
  .composer-actions { display: flex; align-items: center; gap: 6px; }
  .follow-up { color: var(--muted); font-family: var(--font-mono); }
  @keyframes blink { 50% { opacity: 0; } }
  @keyframes breathe { 50% { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) { .caret, .thinking path { animation: none; } }
</style>

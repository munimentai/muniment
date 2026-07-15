<script>
  import { onMount, tick } from 'svelte'

  import { accessErrorState, accessIdleState, accessLoadingState, accessReadyState, bootState, devicesErrorState, devicesIdleState, devicesLoadingState, devicesReadyState, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { ringPath } from './lib/mark.js'
  import { applyBufferedChatEvents, applyChatEvent, composerAction, historyMessages, receiptParts, receiptRows, toolName, toolStatus } from './lib/chat-state.js'
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
  let access = $state(accessIdleState)
  let devices = $state(devicesIdleState)
  let profileSnapshot = $state(null)
  let accessOpen = $state(false)
  let expandedGroups = $state(new Set())
  let profileButton = $state()
  let accessPopover = $state()
  let buffered = new Map()
  let unlisten
  let thread = $state()
  let pinned = $state(true)
  let hasContentBelow = $state(false)
  let lastScrollTop = 0
  let expandedReceipts = $state(new Set())
  let parallelTools = $state(new Map())

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

  $effect(() => {
    const next = new Map(parallelTools)
    for (const message of messages) {
      if (message.role !== 'assistant') continue
      const running = (message.run.toolActivity ?? []).filter((tool) => tool.status === 'running')
      if (running.length > 1) {
        const grouped = new Set(next.get(message.run.id) ?? [])
        running.forEach((tool) => grouped.add(tool.effectId))
        next.set(message.run.id, [...grouped])
      }
    }
    if ([...next].some(([id, tools]) => tools.length !== (parallelTools.get(id)?.length ?? 0))) parallelTools = next
  })

  async function run(action) {
    const command = {
      status: 'auth_status',
      'sign-in': 'auth_sign_in',
      'sign-out': 'auth_sign_out',
    }[action]

    if (action === 'sign-in' || action === 'sign-out') {
      profileSnapshot = null
      access = accessIdleState
      devices = devicesIdleState
      expandedGroups = new Set()
    }
    if (action === 'sign-in') auth = waitingState()
    try {
      const status = await tauri.invoke(command)
      auth = statusState(status)
      if (auth.name === 'signed-in') await Promise.all([loadHistory(), loadAccess()])
    } catch (err) {
      auth = errorState(action, err)
    }
  }

  async function loadHistory() {
    historyError = ''
    expandedReceipts = new Set()
    try {
      const history = await tauri.invoke('chat_history')
      messages = historyMessages(history)
      pinned = true
      followNewContent()
    } catch (_) {
      historyError = 'Conversation history could not be restored. Try again.'
    }
  }

  async function loadAccess(open = false) {
    if (open) accessOpen = true
    access = accessLoadingState()
    expandedGroups = new Set()
    if (open) requestAnimationFrame(() => accessPopover?.focus())
    try {
      const snapshot = await tauri.invoke('auth_entitlement_snapshot')
      profileSnapshot = snapshot
      access = accessReadyState(snapshot)
    } catch (err) {
      access = accessErrorState(err)
    }
  }

  function openAccess() {
    loadAccess(true)
    loadDevices()
  }

  async function loadDevices() {
    devices = devicesLoadingState()
    try {
      devices = devicesReadyState(await tauri.invoke('auth_devices'))
    } catch (_) {
      devices = devicesErrorState()
    }
  }

  function lastActive(value) {
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value))
  }

  function closeAccess() {
    if (!accessOpen) return
    accessOpen = false
    profileButton?.focus()
  }

  function toggleGroup(index) {
    const next = new Set(expandedGroups)
    next.has(index) ? next.delete(index) : next.add(index)
    expandedGroups = next
  }

  onMount(() => {
    if (tauri) run('status')
    window.__TAURI__?.event?.listen('chat-event', ({ payload }) => {
      if (!messages.some((message) => message.run?.id === payload.runId)) {
        buffered.set(payload.runId, [...(buffered.get(payload.runId) ?? []), payload])
        return
      }
      const current = messages.find((message) => message.run?.id === payload.runId)?.run
      const projected = applyChatEvent(current, payload)
      if (projected) messages = messages.map((message) => message.run?.id === projected.id ? { ...message, run: projected } : message)
      if (active?.id === payload.runId) active = projected && !['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? projected : null
    }).then((stop) => { unlisten = stop })
    const outside = (event) => {
      if (accessOpen && !accessPopover?.contains(event.target) && !profileButton?.contains(event.target)) closeAccess()
    }
    const escape = (event) => {
      if (accessOpen && event.key === 'Escape') {
        event.preventDefault()
        closeAccess()
      }
    }
    document.addEventListener('click', outside)
    document.addEventListener('keydown', escape)
    return () => {
      unlisten?.()
      document.removeEventListener('click', outside)
      document.removeEventListener('keydown', escape)
    }
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

  async function resume(run) {
    if (active || !run.resumable || run.phase !== 'interrupted') return
    const resuming = { ...run, phase: 'resuming', resumeError: '' }
    active = resuming
    messages = messages.map((message) => message.run?.id === run.id ? { ...message, run: resuming } : message)
    try {
      await tauri.invoke('chat_resume', { runId: run.id })
      const early = buffered.get(run.id) ?? []
      // Live events can arrive while invoke is still waiting for the durable
      // run.resumed transition. Reconcile from that newer projection instead
      // of restoring the stale, local `resuming` snapshot.
      const current = messages.find((message) => message.run?.id === run.id)?.run ?? resuming
      const projected = applyBufferedChatEvents(current, early)
      buffered.delete(run.id)
      messages = messages.map((message) => message.run?.id === run.id ? { ...message, run: projected } : message)
      active = ['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? null : projected
    } catch (error) {
      const interrupted = { ...run, phase: 'interrupted', resumeError: typeof error === 'string' ? error : 'This reply could not be resumed. Try again.' }
      messages = messages.map((message) => message.run?.id === run.id ? { ...message, run: interrupted } : message)
      active = null
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
            <button bind:this={profileButton} class="profile-button" aria-haspopup="dialog" aria-expanded={accessOpen} onclick={() => accessOpen ? closeAccess() : openAccess()}><span><strong>{profileSnapshot?.user_display_name ?? auth.subject ?? 'Signed in'}</strong><small>{profileSnapshot ? `${profileSnapshot.organization_display_name ?? profileSnapshot.org_id} · ${profileSnapshot.role}` : 'Access unavailable'}</small></span></button>
            {#if accessOpen}
              <div bind:this={accessPopover} class="access-popover" role="dialog" aria-label="Your access" tabindex="-1">
                <header><div><h2>Your access</h2>{#if access.name === 'ready'}<p>Snapshot v{access.snapshot.snapshot_version}</p>{/if}</div><button class="quiet close-access" aria-label="Close your access" onclick={closeAccess}>×</button></header>
                {#if access.name === 'loading'}
                  <p class="access-status" aria-live="polite">Checking your current access…</p>
                {:else if access.name === 'error'}
                  <div class="access-status" role="alert"><p>Your access could not be loaded.</p><button onclick={openAccess}>Try again</button></div>
                {:else if access.name === 'ready'}
                  <p class="access-label">Your groups</p>
                  {#if access.groups.length === 0}<p class="empty-grant">No groups granted</p>{/if}
                  {#each access.groups as group, index}
                    {@const open = expandedGroups.has(index)}
                    <div class="access-group">
                      <button class="group-toggle" aria-expanded={open} aria-controls={`access-group-${index}`} onclick={() => toggleGroup(index)}><span>{group.name}</span><span aria-hidden="true">{open ? '−' : '+'}</span></button>
                      {#if open}<div class="grant-grid" id={`access-group-${index}`}>
                        {#each [['Models', group.models], ['Connections', group.connections], ['Capabilities', group.capabilities]] as category}
                          <div><h3>{category[0]}</h3>{#if category[1].length}<ul>{#each category[1] as item}<li>{item}</li>{/each}</ul>{:else}<p class="empty-grant">None granted</p>{/if}</div>
                        {/each}
                      </div>{/if}
                    </div>
                  {/each}
                {/if}
                <section class="devices-section" aria-labelledby="devices-heading">
                  <h3 id="devices-heading" class="access-label">Devices</h3>
                  {#if devices.name === 'loading'}
                    <p class="access-status" aria-live="polite">Loading devices…</p>
                  {:else if devices.name === 'error'}
                    <div class="access-status" role="alert"><p>Devices could not be loaded.</p><button onclick={loadDevices}>Try again</button></div>
                  {:else if devices.name === 'ready'}
                    {#if devices.devices.length === 0}<p class="empty-grant">No devices found</p>{/if}
                    <ul class="device-list">
                      {#each devices.devices as device (device.device_id)}
                        <li class:revoked={device.revoked_at}>
                          <div class="device-heading"><strong>{device.platform}</strong>{#if device.current}<span class="current-device">This device</span>{/if}<span class="device-state">{device.revoked_at ? 'Revoked' : 'Active'}</span></div>
                          <time datetime={device.last_active_at}>Last active {lastActive(device.last_active_at)}</time>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </section>
                <footer>Access is set by your admins.</footer>
                <button class="quiet sign-out" onclick={() => { closeAccess(); run('sign-out') }}>Sign out</button>
              </div>
            {/if}
          </div>
        </aside>
        <div class="thread-shell">
        <div class="thread" aria-live="polite" bind:this={thread} onscroll={handleThreadScroll}>
          {#if historyError}<p class="history-error" role="alert">{historyError} <button onclick={loadHistory}>Try again</button></p>{/if}
          {#if messages.length === 0}<p class="empty">Ask anything. Your org's routing decides which model answers.</p>{/if}
          {#each messages as message}
            {#if message.role === 'user'}<p class="user-message">{message.text}</p>
            {:else}
            {@const activity = message.run.toolActivity ?? []}
            {@const groupedIds = parallelTools.get(message.run.id) ?? []}
            {@const groupedTools = activity.filter((tool) => groupedIds.includes(tool.effectId))}
            {@const singleTools = activity.filter((tool) => !groupedIds.includes(tool.effectId))}
            <div class="response">
              {#if message.run.phase === 'thinking'}
                <span class="thinking"><svg width="17" height="17" viewBox="0 0 48 48" aria-label="Thinking"><path d={markD} stroke-width="5" /></svg><span>Routing</span></span>
              {:else}<p class:streaming={message.run.phase === 'streaming'}>{message.run.text}{#if message.run.phase === 'streaming'}<span class="caret" aria-hidden="true"></span>{/if}</p>{/if}
              {#if message.run.phase === 'failed'}<div class="run-error">Reply failed. <button onclick={() => { draft = message.run.prompt; send() }}>Try again</button></div>{/if}
              {#if message.run.phase === 'interrupted'}<div class="run-error" role={message.run.resumeError ? 'alert' : undefined}>{message.run.resumeError ?? 'Reply interrupted.'} {#if message.run.resumable}<button disabled={!!active} onclick={() => resume(message.run)}>Resume</button>{:else if message.run.prompt}<button onclick={() => { draft = message.run.prompt; send() }}>Try again</button>{/if}</div>{/if}
              {#if groupedTools.length}
                <div class="tool-card tool-group" role="group" aria-label={`Parallel tool activity: ${groupedTools.map((tool) => `${toolName(tool)} ${toolStatus(tool)}`).join(', ')}`}>
                  <div class="tool-group-title">Parallel tool activity</div>
                  {#each groupedTools as tool}
                    <div class:tool-running={toolStatus(tool) === 'running'} class:tool-failed={toolStatus(tool) === 'failed'} class="tool-row" aria-label={`${toolName(tool)}: ${toolStatus(tool)}`}>
                      <span class="tool-dot" aria-hidden="true"></span><span class="tool-name">{toolName(tool)}</span><span class="tool-status">{toolStatus(tool)}</span>
                    </div>
                  {/each}
                </div>
              {/if}
              {#each singleTools as tool}
                <div class:tool-running={toolStatus(tool) === 'running'} class:tool-failed={toolStatus(tool) === 'failed'} class="tool-card tool-row" role="status" aria-label={`${toolName(tool)}: ${toolStatus(tool)}`}>
                  <span class="tool-dot" aria-hidden="true"></span><span class="tool-name">{toolName(tool)}</span><span class="tool-status">{toolStatus(tool)}</span>
                </div>
              {/each}
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
          <textarea bind:value={draft} onkeydown={keydown} rows="2" placeholder={active?.phase === 'resuming' ? 'Resuming interrupted reply…' : 'Ask anything'} disabled={active?.phase === 'resuming'}></textarea>
          {#if cancelError}<p class="cancel-error" role="alert">{cancelError}</p>{/if}
          {#if queueError}<p class="cancel-error" role="alert">{queueError}</p>{/if}
          <div class="composer-row">
            <span>{active?.phase === 'resuming' ? 'Reopening the existing secure session…' : active && active.id !== 'pending' ? '⏎ steers this reply · queue as follow-up' : 'Routing is automatic. Every reply carries its receipt.'}</span>
            <div class="composer-actions">
              {#if active?.phase === 'resuming'}
                <button disabled>Resuming…</button>
              {:else if active && active.id !== 'pending'}
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
  .profile-block { position: relative; margin-top: auto; padding-top: 10px; border-top: 1px solid var(--border); }
  .profile-button { padding-block: 9px; }
  .profile-button > span:last-child { min-width: 0; display: grid; }
  .profile-button strong { overflow: hidden; text-overflow: ellipsis; font-size: var(--text-13); }
  .profile-button small { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-popover { position: absolute; z-index: 5; left: 0; bottom: calc(100% + 8px); width: 330px; max-height: min(560px, 70vh); overflow-y: auto; padding: 14px; background: var(--paper); border: 1px solid var(--border); border-radius: 10px; box-shadow: 0 12px 36px color-mix(in srgb, var(--ink) 14%, transparent); outline: none; }
  .access-popover:focus-visible { border-color: var(--muted); }
  .access-popover header { display: flex; align-items: start; justify-content: space-between; margin-bottom: 14px; }
  .access-popover h2 { margin: 0; font-size: var(--text-13); }
  .access-popover header p, .access-label { margin: 3px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .close-access { padding: 0 4px; font-size: 18px; }
  .access-label { margin: 0 0 6px; text-transform: uppercase; letter-spacing: .04em; }
  .access-group { border-top: 1px solid var(--border); }
  .group-toggle { width: 100%; display: flex; justify-content: space-between; padding: 9px 2px; border: 0; background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); text-align: left; }
  .grant-grid { display: grid; gap: 10px; padding: 1px 2px 12px 12px; }
  .grant-grid h3 { margin: 0 0 3px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .grant-grid ul { margin: 0; padding: 0; list-style: none; font: var(--text-12) var(--font-mono); }
  .grant-grid li + li { margin-top: 2px; }
  .empty-grant, .access-status { margin: 8px 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-status p { margin: 0 0 6px; }
  .devices-section { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .device-list { margin: 0; padding: 0; list-style: none; }
  .device-list li { padding: 9px 2px; border-top: 1px solid var(--border); }
  .device-list li:first-child { border-top: 0; }
  .device-heading { display: flex; align-items: center; gap: 7px; font-size: var(--text-12); text-transform: capitalize; }
  .device-heading strong { font-weight: 600; }
  .current-device { padding: 1px 5px; border: 1px solid var(--signal); border-radius: 6px; color: var(--signal); font: 10px var(--font-mono); text-transform: none; }
  .device-state { margin-left: auto; color: var(--signal); font: var(--text-12) var(--font-mono); text-transform: none; }
  .device-list .revoked .device-state { color: var(--muted); }
  .device-list time { display: block; margin-top: 3px; color: var(--muted); font: 11px var(--font-mono); }
  .access-popover footer { margin: 12px -14px 0; padding: 11px 14px 0; border-top: 1px solid var(--border); color: var(--muted); font-size: 11px; }
  .sign-out { margin-top: 8px; padding: 2px 0; color: var(--muted); }
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
  .tool-card { margin-top: 8px; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: 12.5px var(--font-mono); }
  .tool-row { display: flex; align-items: center; gap: 8px; min-height: 20px; }
  .tool-group-title { margin-bottom: 4px; color: var(--muted); }
  .tool-group .tool-row + .tool-row { margin-top: 4px; }
  .tool-dot { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 50%; background: currentColor; }
  .tool-name { min-width: 0; overflow-wrap: anywhere; }
  .tool-status { margin-left: auto; }
  .tool-running { color: var(--signal); }
  .tool-running .tool-dot { animation: tool-pulse 1.4s ease-in-out infinite; }
  .tool-failed .tool-status::before { content: 'error · '; }
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
  @keyframes tool-pulse { 50% { opacity: .3; transform: scale(.75); } }
  @media (prefers-reduced-motion: reduce) { .caret, .thinking path, .tool-running .tool-dot { animation: none; } }
</style>

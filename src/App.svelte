<script>
  import { onMount } from 'svelte'
  import { bootState, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { ringPath } from './lib/mark.js'

  const markD = ringPath()
  const tauri = window.__TAURI__?.core
  const events = window.__TAURI__?.event
  let auth = $state(bootState)
  let draft = $state('')
  let messages = $state([])
  let threadId = $state(null)
  let running = $state(false)
  let notice = $state('')

  async function run(action) {
    const command = { status: 'auth_status', 'sign-in': 'auth_sign_in', 'sign-out': 'auth_sign_out' }[action]
    if (action === 'sign-in') auth = waitingState()
    try {
      const status = await tauri.invoke(command)
      auth = statusState(status)
      if (action === 'sign-out') { messages = []; threadId = null }
    } catch (err) { auth = errorState(action, err) }
  }

  async function send() {
    const content = draft.trim()
    if (!content || running) return
    const requestId = crypto.randomUUID()
    draft = ''; notice = ''; running = true
    messages = [...messages, { role: 'user', text: content }, { role: 'assistant', text: '', requestId, streaming: true }]
    try {
      threadId = await tauri.invoke('chat_send', { requestId, threadId, content })
    } catch (error) {
      messages = messages.filter((message) => message.requestId !== requestId)
      notice = String(error)
      running = false
    }
  }

  function keydown(event) {
    if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); send() }
  }

  onMount(() => {
    if (tauri) run('status')
    if (!events) return
    const removers = []
    Promise.all([
      events.listen('chat://delta', ({ payload }) => {
        messages = messages.map((message) => message.requestId === payload.request_id ? { ...message, text: message.text + payload.delta } : message)
      }),
      events.listen('chat://done', ({ payload }) => {
        messages = messages.map((message) => message.requestId === payload.request_id
          ? { ...message, streaming: false, route: payload.route, model: payload.model } : message)
        running = false
      }),
    ]).then((values) => removers.push(...values))
    return () => removers.forEach((remove) => remove())
  })
</script>

{#if tauri && auth.name === 'signed-in'}
  <main class="shell">
    <aside>
      <div class="lockup"><svg width="22" height="22" viewBox="0 0 48 48" aria-hidden="true"><path d={markD} stroke-width="5" /></svg><span>muniment</span></div>
      <button class="new-thread" onclick={() => { messages = []; threadId = null; notice = '' }}>New thread <kbd>⌘N</kbd></button>
      <div class="threads"><p class="label">THREADS</p>{#if threadId}<button class="active-thread">{messages[0]?.text.slice(0, 34) || 'New thread'}</button>{/if}</div>
      <div class="profile"><span>{auth.subject}</span><span class="profile-record">signed in · local session</span><button onclick={() => run('sign-out')}>Sign out</button></div>
    </aside>
    <section class="workspace">
      <header><span>{threadId ? messages[0]?.text.slice(0, 48) : 'New thread'}</span><span class="header-record">⌘K</span></header>
      <div class="conversation">
        <div class="turns" aria-live="polite">
          {#if messages.length === 0}<p class="empty">Ask anything. Your org’s routing decides which model answers.</p>{/if}
          {#each messages as message}
            {#if message.role === 'user'}<div class="user-message">{message.text}</div>
            {:else}<article class:streaming={message.streaming}><div class="response">{message.text}{#if message.streaming}<span class="caret" aria-hidden="true"></span>{/if}</div>{#if message.route}<p class="provenance"><span>{message.route} → {message.model}</span> · routed with your access</p>{/if}</article>{/if}
          {/each}
          {#if notice}<p class="error-record" role="alert">{notice} · Try again.</p>{/if}
        </div>
        <div class="composer-wrap">
          <div class="composer"><textarea bind:value={draft} onkeydown={keydown} disabled={running} aria-label="Message" placeholder="Ask anything…"></textarea><div class="actions"><span class="quiet">Routing is automatic. Every reply carries its receipt.</span><button class="send" onclick={send} disabled={running || !draft.trim()}>Send</button></div></div>
        </div>
      </div>
    </section>
  </main>
{:else}
  <main class="auth-shell">
    <div class="auth-lockup"><svg width="34" height="34" viewBox="0 0 48 48" role="img" aria-label="muniment"><path d={markD} stroke-width="4.5" /></svg><span>muniment</span></div>
    {#if tauri}
      {#if auth.name === 'signed-out'}<p>Sign in to continue to your workspace.</p><button onclick={() => run('sign-in')}>Sign in</button>
      {:else if auth.name === 'signing-in'}<button disabled>Sign in</button><p class="record">Waiting for the browser sign-in…</p>
      {:else if auth.name === 'error'}<p class="record error-record">{auth.message}</p><button onclick={() => run(auth.retry)}>Try again</button>{/if}
    {/if}
  </main>
{/if}

<style>
  button, textarea { font: inherit; color: inherit } button { cursor: pointer }
  svg path { fill: none; stroke: var(--ink); stroke-linecap: round }
  .shell { height: 100vh; display: grid; grid-template-columns: 252px 1fr; overflow: hidden }
  aside { background: var(--surface); border-right: 1px solid var(--border); padding: 21px 14px 16px; display: flex; flex-direction: column }
  .lockup { display: flex; align-items: center; gap: 9px; padding: 0 8px 25px; font-size: 17px; font-weight: 600; letter-spacing: -.01em }
  .new-thread, .active-thread { width: 100%; border: 0; background: transparent; text-align: left; border-radius: var(--radius-control); padding: 8px 9px }
  .new-thread:hover, .active-thread:hover { background: var(--faint) } kbd { float: right; font-family: var(--font-mono); font-size: 11px; color: var(--muted) }
  .threads { margin-top: 25px }.label { padding: 0 9px 7px; font: 11px var(--font-mono); letter-spacing: .04em; color: var(--muted) }.active-thread { background: var(--faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
  .profile { margin-top: auto; border-top: 1px solid var(--border); padding: 15px 8px 0; display: grid; gap: 2px; overflow: hidden }.profile > span { overflow: hidden; text-overflow: ellipsis }.profile-record, .record { font: 11.5px var(--font-mono); color: var(--muted) }.profile button { justify-self: start; border: 0; background: none; padding: 7px 0 0; font-size: 13px; color: var(--muted) }
  .workspace { min-width: 0; display: grid; grid-template-rows: 52px 1fr } header { border-bottom: 1px solid var(--border); display: flex; align-items: center; justify-content: space-between; padding: 0 22px; font-size: 13px }.header-record { font: 11px var(--font-mono); color: var(--muted) }
  .conversation { min-height: 0; display: grid; grid-template-rows: 1fr auto }.turns { width: min(760px, calc(100% - 64px)); margin: 0 auto; padding: 55px 0 30px; overflow-y: auto }.empty { color: var(--muted); margin-top: 18vh }.user-message { margin: 22px 0 26px auto; width: fit-content; max-width: 78%; padding: 9px 13px; background: var(--faint); border-radius: var(--radius-panel); white-space: pre-wrap }.response { display: inline; white-space: pre-wrap }.streaming .response { border-bottom: 2px solid var(--signal) }.caret { display: inline-block; height: 1.05em; border-right: 2px solid var(--signal); margin-left: 2px; vertical-align: -.15em; animation: blink 850ms step-end infinite }.provenance { margin-top: 9px; font: 11.5px var(--font-mono); color: var(--muted) }.provenance span { color: var(--signal) } article + .user-message { margin-top: 34px }.error-record { margin-top: 18px; font: 12px var(--font-mono); color: var(--oxide) }
  .composer-wrap { padding: 0 32px 28px }.composer { width: min(760px, 100%); margin: auto; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); padding: 12px 12px 10px }.composer:focus-within { border-color: var(--muted) }.composer textarea { width: 100%; min-height: 54px; max-height: 210px; resize: vertical; border: 0; outline: 0; background: transparent; line-height: 1.5 }.actions { display: flex; align-items: center; justify-content: space-between; gap: 15px }.quiet { font-size: 11px; color: var(--muted) }.send, .auth-shell button { border: 1px solid var(--ink); border-radius: var(--radius-control); background: var(--ink); color: var(--paper); padding: 5px 12px }.send:disabled { opacity: .4; cursor: default }
  .auth-shell { min-height: 100vh; display: grid; place-content: center; justify-items: center; gap: 18px }.auth-lockup { display: flex; gap: 13px; align-items: center; font-size: 30px; font-weight: 600; letter-spacing: -.01em }.auth-shell p { color: var(--muted) }
  button:focus-visible, textarea:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px }
  @keyframes blink { 50% { opacity: 0 } }
  @media (prefers-reduced-motion: reduce) { .caret { animation: none } }
</style>

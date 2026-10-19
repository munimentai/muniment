<script>
  import FailureNote from './FailureNote.svelte'
  import DisclosureSummary from './ui/DisclosureSummary.svelte'
  import AllowanceMeter from './AllowanceMeter.svelte'
  import { allowanceLabel } from './allowance-label.js'
  // One provider's accounts in the router's pool. A subscription is a card with
  // what it has left and what it has served, and it holds its own Disconnect. A
  // key is a one-line card with edit and disconnect icons. The sign-ins the
  // pool takes and a key form follow. Every command answers with the whole
  // router settings, and the parent redraws from them. A shared sign-in
  // names the fleet member that holds it, from `fleet`.
  import { onDestroy, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import Toggle from './Toggle.svelte'
  import HoldSignIn from './HoldSignIn.svelte'
  import { routerHold } from './fleet-accounts.js'

  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args), settings, family, onsettings, fleet = null, onheld } = $props()

  let editingId = $state(null)
  let nameDraft = $state('')
  let nameInput = $state()
  async function editName(account) {
    editingId = account.id
    nameDraft = account.label
    await tick()
    nameInput?.focus()
    nameInput?.select()
  }
  async function saveName(account) {
    if (!nameDraft.trim() || pending) return
    await run('model_router_update_account', { id: account.id, label: nameDraft.trim() })
    if (!formError) editingId = null
  }
  let status = $state('')
  let formError = $state('')
  let pending = $state(false)
  let adding = $state(false)
  let label = $state('')
  let key = $state('')
  let baseUrl = $state('')
  let signIn = $state(null)
  // The sign-in page's address stays off the page. Copy puts it on the clipboard.
  let signInLinkCopied = $state(false)
  async function copySignInLink() {
    try {
      await navigator.clipboard.writeText(signIn.url)
      signInLinkCopied = true
      setTimeout(() => { signInLinkCopied = false }, 1500)
    } catch {
      signIn = { ...signIn, message: 'The link could not be copied.' }
    }
  }
  let signInAnswer = $state('')
  let unlisten = null
  onDestroy(() => stopListening())

  const accounts = $derived((settings?.accounts ?? []).filter((account) => account.family === family))
  const subscriptions = $derived(accounts.filter((account) => account.source !== 'key'))
  const keys = $derived(accounts.filter((account) => account.source === 'key'))
  const signIns = $derived((settings?.subscriptions ?? []).filter((entry) => entry.family === family))
  const familyRow = $derived((settings?.families ?? []).find((entry) => entry.id === family) ?? null)
  const busiest = $derived(Math.max(1, ...(settings?.accounts ?? []).flatMap((account) => account.days.map((day) => day[1]))))

  async function run(command, payload, done = '') {
    pending = true
    formError = ''
    try {
      const updated = await tauri.invoke(command, payload)
      onsettings?.(updated)
      status = done
    } catch (error) {
      formError = String(error?.message ?? error)
    } finally {
      pending = false
    }
  }

  function stopListening() {
    if (unlisten) {
      const stop = unlisten
      unlisten = null
      void Promise.resolve(stop).then((fn) => fn?.())
    }
  }

  // A subscription sign-in: Pi's own flow, or the router's own for Kimi,
  // Antigravity and Devin. Either way the credential joins this pool.
  async function startSignIn(entry) {
    if (pending) return
    pending = true
    formError = ''
    signInAnswer = ''
    signIn = { provider: entry.provider, label: entry.label, stage: 'start', message: 'Starting the sign-in…', url: '', code: '', prompt: null }
    stopListening()
    unlisten = listen('local-mode-login', ({ payload }) => onSignInEvent(payload))
    try {
      await tauri.invoke('model_router_subscription_start', { provider: entry.provider })
    } catch (error) {
      signIn = { ...signIn, stage: 'failed', message: String(error?.message ?? error) }
      stopListening()
    } finally {
      pending = false
    }
  }

  function onSignInEvent(payload) {
    if (!signIn || (payload.provider && payload.provider !== signIn.provider && payload.stage !== 'cancelled')) return
    switch (payload.stage) {
      case 'event': {
        const event = payload.event ?? {}
        if (event.type === 'auth_url') {
          signIn = { ...signIn, stage: 'browser', url: event.url, message: event.instructions ?? 'Continue in your browser.' }
          void tauri.invoke('local_mode_open_url', { url: event.url }).catch(() => {})
        } else if (event.type === 'device_code') {
          signIn = { ...signIn, stage: 'code', url: event.verificationUriComplete ?? event.verificationUri ?? '', code: event.userCode ?? '', message: 'Enter this code on the sign-in page.' }
          if (signIn.url) void tauri.invoke('local_mode_open_url', { url: signIn.url }).catch(() => {})
        } else if (event.message) {
          signIn = { ...signIn, message: event.message }
        }
        break
      }
      case 'prompt':
        signIn = { ...signIn, prompt: payload }
        break
      case 'done': {
        stopListening()
        const name = signIn?.label ?? 'The'
        signIn = null
        status = `${name} account is in the pool. Its allowance appears after the first probe.`
        void reload()
        // The probe that names the account runs after the sign-in, so read again.
        setTimeout(() => { void reload() }, 4000)
        break
      }
      case 'failed':
        stopListening()
        signIn = { ...signIn, stage: 'failed', message: payload.message || 'The sign-in did not finish. Try again.', prompt: null }
        break
      case 'cancelled':
        stopListening()
        signIn = { ...signIn, stage: 'cancelled', message: 'The sign-in was cancelled.', prompt: null }
        break
      case 'exit':
        if (signIn && signIn.stage !== 'failed' && signIn.stage !== 'cancelled') {
          stopListening()
          signIn = { ...signIn, stage: 'failed', message: 'The sign-in stopped before it finished. Try again.', prompt: null }
        }
        break
      default:
        if (payload.message) signIn = { ...signIn, message: payload.message }
    }
  }

  async function reload() {
    try {
      onsettings?.(await tauri.invoke('model_router_settings'))
    } catch (_) {}
  }

  async function answerSignIn(value) {
    const prompt = signIn?.prompt
    if (!prompt) return
    try {
      await tauri.invoke('local_mode_account_login_answer', { id: prompt.id, value, confirmed: null })
      signIn = { ...signIn, prompt: null }
      signInAnswer = ''
    } catch (error) {
      formError = String(error?.message ?? error)
    }
  }

  async function cancelSignIn() {
    try { await tauri.invoke('local_mode_account_login_cancel') } catch (_) {}
  }

  // A key serves every model its provider lists.
  function addAccount() {
    void run('model_router_add_account', { family, label, key, baseUrl: baseUrl.trim() || null, models: [] }, `${label} is in the ${familyRow?.name ?? family} pool.`)
      .then(() => { if (!formError) { label = ''; key = ''; baseUrl = ''; adding = false } })
  }

  function refreshQuota(account) {
    void run('model_router_refresh_quota', { id: account.id }, `${account.label} refreshed.`)
  }

  function until(ms) {
    if (!ms) return ''
    const seconds = Math.round((ms - Date.now()) / 1000)
    if (seconds <= 0) return 'now'
    if (seconds < 3600) return `in ${Math.max(1, Math.round(seconds / 60))}m`
    if (seconds < 86400) return `in ${Math.round(seconds / 3600)}h`
    const days = Math.floor(seconds / 86400)
    const hours = Math.round((seconds % 86400) / 3600)
    return `in ${days}d ${hours}h`
  }

  function when(ms) {
    if (!ms) return 'never'
    const seconds = Math.round((Date.now() - ms) / 1000)
    if (seconds < 60) return 'just now'
    if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`
    if (seconds < 86400) return `${Math.round(seconds / 3600)}h ago`
    return `${Math.round(seconds / 86400)}d ago`
  }

  function tokens(count) {
    if (count < 1000) return String(count)
    if (count < 1_000_000) return `${(count / 1000).toFixed(1)}K`
    return `${(count / 1_000_000).toFixed(2)}M`
  }

  function cooling(account) {
    return account.cooldown_until_ms && account.cooldown_until_ms > Date.now()
  }
</script>

<div class="accounts" aria-label={`${familyRow?.name ?? family} accounts`}>
  {#if status}<p class="support" role="status">{status}</p>{/if}
  {#if formError}<p class="support" role="alert">{formError}</p>{/if}
  {#snippet routingNote(account)}
    {#if !account.enabled || account.servable === false || cooling(account) || account.weight === 0}
      <p class="support">{!account.enabled ? 'Disabled' : account.servable === false ? 'Routing unavailable' : cooling(account) ? `Rate limited · retry ${until(account.cooldown_until_ms)}` : 'Excluded from routing'}
        <!-- Every account routes. One switched off or set to no share before the card dropped those controls gets one way back. -->
        {#if !account.enabled || account.weight === 0}<button type="button" class="quiet small" disabled={pending} onclick={() => run('model_router_update_account', { id: account.id, enabled: true, weight: 1 })}>Turn on</button>{/if}
      </p>
    {/if}
  {/snippet}
  {#if subscriptions.length}
    <ul class="cards">
      {#each subscriptions as account (account.id)}
        <li class="card" class:cooling={cooling(account)}>
          <header>
            <span class="card-label">{account.label}</span>
            <span class="tag">{account.plan || 'Subscription'}</span>
          </header>
          {#if routerHold(fleet, account.id)}<HoldSignIn {tauri} account={routerHold(fleet, account.id)} onmoved={onheld} />{/if}
          {@render routingNote(account)}
          {#if !account.allowance_readable}
            <p class="record">Allowance unavailable.</p>
          {:else if account.windows.length === 0}
            <p class="record">{account.quota_observed_ms ? 'No allowance reported.' : 'Allowance pending.'}</p>
          {/if}
          {#each account.windows as window (window.label + window.scope)}
            <div class="window">
              <div class="window-head">
                <span class="record">{allowanceLabel(window)}</span>
                <span class="record"><strong>{Math.round(window.remaining_percent)}%</strong> left{#if window.resets_at_ms}{` · resets ${until(window.resets_at_ms)}`}{/if}</span>
              </div>
              <AllowanceMeter remaining={window.remaining_percent} limited={window.limit_reached} />
            </div>
          {/each}
          {#if account.banked_resets}<p class="record">{account.banked_resets} banked {account.banked_resets === 1 ? 'reset' : 'resets'}</p>{/if}
          <details class="account-details"><DisclosureSummary>Usage and settings</DisclosureSummary>
          {#if (account.email && account.email !== account.label) || account.models.length}
          <p class="record tier">{[account.email !== account.label ? account.email : null, ...account.models].filter(Boolean).join(' · ')}</p>
          {/if}
          <div class="bars" aria-label={`${account.label} turns per day`}>
            {#each account.days as [day, requests] (day)}
              <span class="bar" style={`height: ${Math.max(2, Math.round((requests / busiest) * 22))}px`} aria-label={`${day}: ${requests} turns`}></span>
            {/each}
            {#if account.days.length === 0}<span class="record">No turn yet.</span>{/if}
          </div>
          <dl class="figures">
            <div><dt>Turns</dt><dd>{account.requests}</dd></div>
            <div><dt>Active</dt><dd>{account.active}</dd></div>
            <div><dt>Errors</dt><dd>{account.errors}</dd></div>
            <div class="wide"><dt>Tokens</dt><dd>{tokens(account.input_tokens)} in · {tokens(account.output_tokens)} out</dd></div>
            <div class="wide"><dt>Last used</dt><dd>{when(account.last_used_ms)}</dd></div>
          </dl>
          {#if account.last_error}<p class="record error">{account.last_error}</p>{/if}
          </details>
          <!-- The allowance reads itself every two minutes. The refresh icon reads it now. -->
          <footer>
            {#if account.allowance_readable}
              <span class="refreshed">{account.quota_observed_ms ? `Updated ${when(account.quota_observed_ms)}` : 'Not read yet'}<button type="button" class="quiet small refresh" aria-label={`Refresh ${account.label} allowance`} disabled={pending} onclick={() => refreshQuota(account)}><LucideIcon name="refresh-cw" size={14} variant="action" /></button></span>
            {/if}
            <button type="button" class="quiet small disconnect" aria-label={`Disconnect ${account.label}`} onclick={() => run('model_router_remove_account', { id: account.id }, `${account.label} is disconnected.`)}><LucideIcon name="unplug" size={14} variant="action" />Disconnect</button>
          </footer>
        </li>
      {/each}
    </ul>
  {/if}

  {#if keys.length}
    <ul class="keys">
      {#each keys as account (account.id)}
        <li class="key-card" class:cooling={cooling(account)}>
          <LucideIcon name="key-round" size={16} />
          {#if editingId === account.id}
            <form class="name-edit" onsubmit={(event) => { event.preventDefault(); void saveName(account) }}>
              <input bind:this={nameInput} bind:value={nameDraft} aria-label="Account name" maxlength="120" disabled={pending} onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); editingId = null } }} />
              <button type="submit" data-variant="primary" disabled={pending || !nameDraft.trim()}>Save</button>
              <button type="button" disabled={pending} onclick={() => editingId = null}>Cancel</button>
            </form>
          {:else}
            <span class="key-text">
              <span class="card-label">{account.label}<span class="tag">API key</span></span>
              {@render routingNote(account)}
            </span>
            <!-- A key never balances with subscriptions. With overflow on, it serves only once every subscription is cooling or at its limit. -->
            {#if subscriptions.length}
              <span class="overflow">Use when subscriptions run out<Toggle checked={!!account.overflow} disabled={pending} aria-label={`Use ${account.label} when subscriptions run out`} onchange={(value) => run('model_router_update_account', { id: account.id, overflow: value }, value ? `${account.label} serves when subscriptions run out.` : `${account.label} no longer serves when subscriptions run out.`)} /></span>
            {/if}
            <button type="button" class="quiet icon-action" aria-label={`Edit ${account.label}`} disabled={pending} onclick={() => editName(account)}><LucideIcon name="square-pen" size={16} variant="action" /></button>
            <button type="button" class="quiet icon-action disconnect" aria-label={`Disconnect ${account.label}`} disabled={pending} onclick={() => run('model_router_remove_account', { id: account.id }, `${account.label} is disconnected.`)}><LucideIcon name="unplug" size={16} variant="action" /></button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if signIn}
    <div class="login" aria-live="polite">
      {#if signIn.stage === 'failed'}<FailureNote failure={signIn.message} detail={signIn.detail} />{:else}<p class="support">{signIn.message}</p>{/if}
      {#if signIn.code}<p class="login-code">{signIn.code}</p>{/if}
      {#if signIn.url}
        <div class="login-links">
          <button type="button" onclick={() => tauri.invoke('local_mode_open_url', { url: signIn.url }).catch(() => {})}>Open again</button>
          <button type="button" class="quiet" onclick={copySignInLink}>{signInLinkCopied ? 'Copied' : 'Copy sign-in link'}</button>
        </div>
      {/if}
      {#if signIn.prompt?.kind === 'select'}
        <p class="support">{signIn.prompt.title}</p>
        <div class="login-options">
          {#each signIn.prompt.options as option}<button type="button" onclick={() => answerSignIn(option)}>{option}</button>{/each}
        </div>
      {:else if signIn.prompt?.kind === 'input'}
        <label for={`pool-sign-in-answer-${family}`}>{signIn.prompt.title}</label>
        <input id={`pool-sign-in-answer-${family}`} type="text" placeholder={signIn.prompt.placeholder ?? ''} bind:value={signInAnswer}>
        <button type="button" disabled={!signInAnswer.trim()} onclick={() => answerSignIn(signInAnswer.trim())}>Continue</button>
      {/if}
      {#if signIn.stage === 'failed' || signIn.stage === 'cancelled'}
        <button type="button" onclick={() => { signIn = null }}>Back</button>
      {:else}
        <button type="button" class="quiet" onclick={cancelSignIn}>Cancel</button>
      {/if}
    </div>
  {:else if adding}
    <form class="add" aria-label={`Add a ${familyRow?.name ?? family} key`} onsubmit={(event) => { event.preventDefault(); addAccount() }}>
      <p class="support">API usage has separate billing from a chat subscription.</p>
      <label for={`router-label-${family}`}>Name</label>
      <input id={`router-label-${family}`} type="text" placeholder="work" bind:value={label} disabled={pending}>
      <label for={`router-key-${family}`}>API key</label>
      <input id={`router-key-${family}`} type="password" autocomplete="off" bind:value={key} disabled={pending}>
      <label for={`router-base-${family}`}>Server URL (optional)</label>
      <input id={`router-base-${family}`} type="url" placeholder={familyRow?.base_url ?? ''} bind:value={baseUrl} disabled={pending}>
      <div class="actions">
        <button type="submit" disabled={pending || !label.trim() || !key.trim()}>Add key</button>
        <button type="button" class="quiet" onclick={() => { adding = false; formError = '' }}>Cancel</button>
      </div>
    </form>
  {:else}
    <div class="sign-ins">
      {#each signIns as entry (entry.provider)}
        <button type="button" class="sign-in" disabled={pending} aria-label={`Subscription: ${entry.label}`} onclick={() => startSignIn(entry)}><LucideIcon name="credit-card" size={14} />{signIns.length === 1 ? 'Subscription' : entry.label}</button>
      {/each}
      {#if family !== 'devin'}
        <button type="button" class="sign-in" disabled={pending} onclick={() => { adding = true }}><LucideIcon name="key-round" size={14} />Add a key</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .small { min-height: 24px; padding: 2px 8px; font-size: var(--text-12); }
  /* Disconnecting drops the account from every pool, so it reads as critical. */
  .disconnect { display: inline-flex; align-items: center; gap: 6px; color: var(--oxide); }
  .accounts { container-type: inline-size; display: grid; gap: 8px; min-width: 0; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); overflow-wrap: anywhere; }
  .tag, .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .error { overflow-wrap: anywhere; }
  /* One card per account: its allowance as bars of what is left, then what it has served. */
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 320px), 1fr)); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .card { display: grid; min-width: 0; gap: 6px; align-content: start; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-control); overflow-wrap: anywhere; }
  .card .support { font-size: var(--text-12); line-height: 1.4; }
  .card p.record { margin: 0; }
  .account-details { margin-top: 2px; border-top: 1px solid var(--border); padding-top: 6px; }
  .account-details summary { cursor: pointer; min-height: 20px; font-size: var(--text-12); }
  .account-details[open] { display: grid; gap: 10px; }
  .card.cooling { border-color: var(--muted); }
  .card header { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 8px; }
  .card-label { flex: 1 1 160px; min-width: 0; font-size: var(--text-13); font-weight: 600; }
  .card-label { display: flex; align-items: center; gap: 4px; }
  .name-edit { display: flex; flex: 1; flex-wrap: wrap; gap: 4px; min-width: 0; }
  /* A key is one line: its name, then edit and disconnect icons at the right. */
  .keys { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .key-card { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; min-width: 0; min-height: 44px; padding: 6px 6px 6px 12px; box-sizing: border-box; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .key-card.cooling { border-color: var(--muted); }
  .key-text { display: grid; flex: 1 1 160px; gap: 2px; min-width: 0; }
  .overflow { display: inline-flex; align-items: center; gap: 8px; color: var(--muted); font-size: var(--text-12); }
  .key-card .card-label { gap: 8px; }
  .key-card .support { font-size: var(--text-12); }
  .icon-action { display: inline-flex; align-items: center; justify-content: center; min-width: 28px; min-height: 28px; padding: 4px; }
  .tier { margin: 0; overflow-wrap: anywhere; }
  .window { display: grid; min-width: 0; gap: 4px; }
  .window-head { display: grid; gap: 2px; }
  .window-head strong { color: var(--ink); font-weight: 600; }
  .bars { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 2px; min-height: 26px; margin-bottom: 10px; }
  .bar { width: 5px; background: var(--muted); }
  /* The three counts share the first row and the two wider figures the second, each value beside its label. */
  .figures { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 8px 10px; margin: 0; }
  .figures div { display: flex; flex-wrap: wrap; align-items: baseline; grid-column: span 2; min-width: 0; gap: 2px 8px; }
  .figures div.wide { grid-column: span 3; }
  .figures dt { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .figures dd { margin: 0; font: var(--text-12) var(--font-mono); }
  .card footer { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding-top: 2px; }
  .refreshed { display: inline-flex; align-items: center; gap: 2px; color: var(--muted); font-size: var(--text-12); }
  .refresh { padding: 2px 4px; }
  .card footer .disconnect { margin-left: auto; }
  .sign-ins { display: flex; flex-wrap: wrap; gap: 6px; }
  .sign-in { display: inline-flex; align-items: center; gap: 8px; min-height: 28px; padding: 4px 10px; font-size: var(--text-12); }
  .login { display: grid; gap: 8px; justify-items: start; }
  .login-code { margin: 0; font: var(--text-22) var(--font-mono); letter-spacing: .08em; }
  .login-links { display: flex; flex-wrap: wrap; gap: 8px; }
  .login-options { display: flex; flex-wrap: wrap; gap: 6px; }
  .add { display: grid; gap: 6px; }
  .actions { display: flex; gap: 6px; }
  label { color: var(--muted); font: var(--text-12) var(--font-mono); }
  input[type="password"], input[type="url"], input[type="text"], input[type="number"], textarea { box-sizing: border-box; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: inherit; font-size: var(--text-13); }
  input[type="password"], input[type="url"], input[type="text"], textarea { width: min(420px, 100%); }
  input:focus, textarea:focus { border-color: var(--muted); outline: 0; }
  textarea { resize: vertical; font-family: var(--font-mono); font-size: var(--text-12); }
</style>

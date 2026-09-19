<script>
  // One provider's accounts in the router's pool: each a named card with what
  // the subscription has left and what the account has served, the sign-ins
  // the pool can take for this provider, and a key form. Every command
  // answers with the whole router settings, and the parent redraws from them.
  import { onDestroy, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import ProviderLogo from './ProviderLogo.svelte'

  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args), settings, family, onsettings } = $props()

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
  let accountModels = $state('')
  let signIn = $state(null)
  let signInAnswer = $state('')
  let unlisten = null
  onDestroy(() => stopListening())

  const accounts = $derived((settings?.accounts ?? []).filter((account) => account.family === family))
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

  function addAccount() {
    const models = accountModels.split(/[\n,]/).map((model) => model.trim()).filter(Boolean)
    void run('model_router_add_account', { family, label, key, baseUrl: baseUrl.trim() || null, models }, `${label} is in the ${familyRow?.name ?? family} pool.`)
      .then(() => { if (!formError) { label = ''; key = ''; baseUrl = ''; accountModels = ''; adding = false } })
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
  {#if accounts.length}
    <ul class="cards">
      {#each accounts as account (account.id)}
        <li class="card" class:cooling={cooling(account)}>
          <header>
            {#if editingId === account.id}
              <form class="name-edit" onsubmit={(event) => { event.preventDefault(); void saveName(account) }}>
                <input bind:this={nameInput} bind:value={nameDraft} aria-label="Account name" maxlength="120" disabled={pending} onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); editingId = null } }} />
                <button disabled={pending || !nameDraft.trim()}>Save</button>
                <button type="button" disabled={pending} onclick={() => editingId = null}>Cancel</button>
              </form>
            {:else}
              <span class="card-label">{account.label}<button type="button" class="rename quiet" aria-label={`Rename ${account.label}`} disabled={pending} onclick={() => editName(account)}><LucideIcon name="pencil" /></button></span>
            {/if}
            <span class="tag">{account.source === 'key' ? 'API key' : 'Subscription'}</span>
          </header>
          <p class="support">{!account.enabled ? 'Disabled' : account.servable === false ? 'Not available for routed turns yet.' : cooling(account) ? 'Waiting for the rate limit to reset.' : account.weight === 0 ? 'Weight is zero. This account receives no turns.' : 'Available for routed turns.'}</p>
          <p class="record tier">{account.source === 'key' ? (account.base_url ?? familyRow?.base_url ?? '') : `${account.plan ?? 'plan not read yet'}`}{#if account.email && account.email !== account.label} · {account.email}{/if}{#if account.models.length} · {account.models.join(' · ')}{/if}</p>
          {#if account.source !== 'key'}
            {#if !account.allowance_readable}
              <p class="record">Muniment cannot read what this account has left.</p>
            {:else if account.windows.length === 0}
              <p class="record">{account.quota_observed_ms ? 'No window reported.' : 'Allowance not read yet.'}</p>
            {/if}
            {#each account.windows as window (window.label + window.scope)}
              <div class="window" class:reached={window.limit_reached}>
                <div class="window-head">
                  <span class="record">{window.label}{#if window.scope} · {window.scope}{/if}</span>
                  <span class="record"><strong>{Math.round(window.remaining_percent)}%</strong> left{#if window.resets_at_ms} · resets {until(window.resets_at_ms)}{/if}</span>
                </div>
                <div class="window-bar"><span style={`width: ${Math.round(window.remaining_percent)}%`}></span></div>
              </div>
            {/each}
            {#if account.quota_observed_ms}<p class="support">Allowance updated {when(account.quota_observed_ms)}.</p>{/if}
            {#if account.banked_resets}<p class="record">{account.banked_resets} banked {account.banked_resets === 1 ? 'reset' : 'resets'}</p>{/if}
          {/if}
          {#if cooling(account)}<p class="tag warn">Rate limited · back in {Math.max(1, Math.round((account.cooldown_until_ms - Date.now()) / 1000))}s</p>{/if}
          <details class="account-details"><summary>Usage and settings</summary>
          <div class="bars" aria-label={`${account.label} turns per day`}>
            {#each account.days as [day, requests] (day)}
              <span class="bar" style={`height: ${Math.max(2, Math.round((requests / busiest) * 22))}px`} aria-label={`${day}: ${requests} turns`}></span>
            {/each}
            {#if account.days.length === 0}<span class="record">No turn yet.</span>{/if}
          </div>
          <dl class="figures">
            <div><dt>Turns</dt><dd>{account.requests}</dd></div>
            <div><dt>Tokens</dt><dd>{tokens(account.input_tokens)} in · {tokens(account.output_tokens)} out</dd></div>
            <div><dt>Active</dt><dd>{account.active}</dd></div>
            <div><dt>Last used</dt><dd>{when(account.last_used_ms)}</dd></div>
            <div><dt>Errors</dt><dd>{account.errors}</dd></div>
            <div><dt>Share</dt><dd><input type="number" min="0" max="100" aria-label={`${account.label} share`} value={account.weight} onchange={(event) => run('model_router_update_account', { id: account.id, weight: Math.max(0, Math.round(Number(event.currentTarget.value) || 0)) })}></dd></div>
          </dl>
          {#if account.last_error}<p class="record error">{account.last_error}</p>{/if}
          <footer>
            <button type="button" class="quiet small" onclick={() => run('model_router_remove_account', { id: account.id }, `${account.label} is removed.`)}>Remove</button>
            {#if account.source !== 'key' && account.allowance_readable}<button type="button" class="quiet small" onclick={() => refreshQuota(account)}>Refresh allowance</button>{/if}
            <button type="button" role="switch" class="switch" aria-checked={account.enabled} aria-label={`Use ${account.label}`} onclick={() => run('model_router_update_account', { id: account.id, enabled: !account.enabled })}><span></span></button>
          </footer>
          </details>
        </li>
      {/each}
    </ul>
  {/if}

  {#if signIn}
    <div class="login" aria-live="polite">
      <p class="support">{signIn.message}</p>
      {#if signIn.code}<p class="login-code">{signIn.code}</p>{/if}
      {#if signIn.url}
        <button type="button" onclick={() => tauri.invoke('local_mode_open_url', { url: signIn.url }).catch(() => {})}>Open the sign-in page</button>
        <p class="record login-url">{signIn.url}</p>
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
      <label for={`router-label-${family}`}>Name</label>
      <input id={`router-label-${family}`} type="text" placeholder="work" bind:value={label} disabled={pending}>
      <label for={`router-key-${family}`}>API key</label>
      <input id={`router-key-${family}`} type="password" autocomplete="off" bind:value={key} disabled={pending}>
      <label for={`router-base-${family}`}>Server URL (optional)</label>
      <input id={`router-base-${family}`} type="url" placeholder={familyRow?.base_url ?? ''} bind:value={baseUrl} disabled={pending}>
      <label for={`router-models-${family}`}>Models this account serves, one per line (optional)</label>
      <textarea id={`router-models-${family}`} rows="2" bind:value={accountModels} disabled={pending}></textarea>
      <p class="support">Leave the model list empty and the account serves every model of its provider.</p>
      <div class="actions">
        <button type="submit" disabled={pending || !label.trim() || !key.trim()}>Add key</button>
        <button type="button" class="quiet" onclick={() => { adding = false; formError = '' }}>Cancel</button>
      </div>
    </form>
  {:else}
    <div class="sign-ins">
      {#each signIns as entry (entry.provider)}
        <button type="button" class="sign-in" disabled={pending} onclick={() => startSignIn(entry)}><ProviderLogo provider={entry.family} size={16} />{entry.label}</button>
      {/each}
      {#if family !== 'devin'}
        <button type="button" class="sign-in" disabled={pending} onclick={() => { adding = true }}>Add a key</button>
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
  .accounts { display: grid; gap: 8px; min-width: 0; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); overflow-wrap: anywhere; }
  .tag, .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .warn { color: var(--ink); }
  .error { overflow-wrap: anywhere; }
  /* One card per account: its allowance as bars of what is left, then what it has served. */
  .cards { display: grid; grid-template-columns: minmax(0, 1fr); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .card { display: grid; min-width: 0; gap: 8px; align-content: start; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-control); overflow-wrap: anywhere; }
  .account-details { border-top: 1px solid var(--border); padding-top: 8px; }
  .account-details summary { cursor: pointer; min-height: 24px; font-size: var(--text-13); }
  .account-details[open] { display: grid; gap: 10px; }
  .card.cooling { border-color: var(--muted); }
  .card header { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 8px; }
  .card-label { flex: 1 1 160px; min-width: 0; font-size: var(--text-13); font-weight: 600; }
  .card-label { display: flex; align-items: center; gap: 4px; }
  .rename { opacity: 0; padding: 4px; }
  .card header:hover .rename, .rename:focus-visible { opacity: 1; }
  .name-edit { display: flex; flex-wrap: wrap; gap: 4px; width: 100%; }
  @media (hover: none) { .rename { opacity: 1; } }
  .tier { margin: 0; overflow-wrap: anywhere; }
  .window { display: grid; min-width: 0; gap: 4px; }
  .window-head { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 2px 8px; }
  .window-head strong { color: var(--ink); font-weight: 600; }
  .window-bar { height: 4px; border-radius: var(--radius-chip); background: var(--faint); }
  .window-bar span { display: block; height: 100%; border-radius: var(--radius-chip); background: var(--ink); }
  .window.reached .window-bar span { background: var(--muted); }
  .bars { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 2px; min-height: 26px; }
  .bar { width: 5px; background: var(--muted); }
  .figures { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px 10px; margin: 0; }
  .figures div { display: grid; min-width: 0; gap: 1px; }
  .figures dt { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .figures dd { margin: 0; font: var(--text-12) var(--font-mono); }
  .figures input { width: 52px; }
  .card footer { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding-top: 2px; }
  .card footer .switch { margin-left: auto; }
  .sign-ins { display: flex; flex-wrap: wrap; gap: 6px; }
  .sign-in { display: inline-flex; align-items: center; gap: 8px; min-height: 28px; padding: 4px 10px; font-size: var(--text-12); }
  .login { display: grid; gap: 8px; justify-items: start; }
  .login-code { margin: 0; font: var(--text-22) var(--font-mono); letter-spacing: .08em; }
  .login-url { max-width: 100%; overflow-wrap: anywhere; }
  .login-options { display: flex; flex-wrap: wrap; gap: 6px; }
  .add { display: grid; gap: 6px; }
  .actions { display: flex; gap: 6px; }
  label { color: var(--muted); font: var(--text-12) var(--font-mono); }
  input[type="password"], input[type="url"], input[type="text"], input[type="number"], textarea { box-sizing: border-box; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: inherit; font-size: var(--text-13); }
  input[type="password"], input[type="url"], input[type="text"], textarea { width: min(420px, 100%); }
  input:focus, textarea:focus { border-color: var(--muted); outline: 0; }
  textarea { resize: vertical; font-family: var(--font-mono); font-size: var(--text-12); }
  .switch { position: relative; flex: none; width: 30px; height: 18px; padding: 0; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--paper); }
  .switch span { position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: var(--radius-chip); background: var(--muted); transition: transform 120ms ease, background 120ms ease; }
  .switch[aria-checked="true"] { border-color: var(--signal); background: var(--signal-soft); }
  .switch[aria-checked="true"] span { transform: translateX(12px); background: var(--signal); }
  .switch:hover:not(:disabled) { background: var(--faint); }
</style>

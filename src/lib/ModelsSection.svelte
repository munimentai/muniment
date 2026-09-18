<script>
  // Settings → Models, one screen: every connection is a named account under
  // its provider, key or subscription, with its allowance and usage on the
  // card, the provider's models once, the connector that adds a provider, and
  // Routing at the foot.
  import { onDestroy } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import ModelAccounts from './ModelAccounts.svelte'
  import ModelRouterSection from './ModelRouterSection.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import { catalogProvider, connectableProviders, familyName, familyProvider, methodLabel, modelKey, providerFamily, providerName, searchProviders, sourceTag } from './provider-catalog.js'

  // `inventory` seeds the list from what the shell already holds, so the page
  // draws at once and the fresh read replaces it.
  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args), oninventory, inventory: initial = null } = $props()

  // svelte-ignore state_referenced_locally
  let inventory = $state(initial)
  let loadError = $state('')
  let status = $state('')
  let query = $state('')
  let view = $state('list')
  // The router's settings: the accounts in every pool, the models in the
  // running and the classifier. One read feeds every card and the Routing foot.
  let router = $state(null)
  // Where Back from a provider's view returns: the list or the connector.
  let origin = $state('list')
  let providerId = $state('')
  let method = $state('')
  let providerQuery = $state('')
  let key = $state('')
  let baseUrl = $state('')
  let endpointName = $state('')
  let endpointModels = $state('')
  let pending = $state(false)
  let formError = $state('')
  let claude = $state(null)
  let login = $state(null)
  let loginAnswer = $state('')
  let unlisten = null

  const provider = $derived(catalogProvider(providerId))
  const catalog = $derived(searchProviders(providerQuery))
  const connectable = $derived(connectableProviders(inventory))
  const hidden = $derived(new Set(inventory?.hidden ?? []))
  const shownProviders = $derived.by(() => {
    if (!inventory) return []
    const needle = query.trim().toLowerCase()
    return inventory.providers
      .filter((entry) => entry.source !== 'router')
      .map((entry) => ({
        ...entry,
        family: providerFamily(entry.id),
        models: needle ? entry.models.filter((model) => model.id.toLowerCase().includes(needle)) : entry.models,
      })).filter((entry) => !needle || entry.models.length > 0 || entry.name.toLowerCase().includes(needle))
  })
  // A family whose accounts sit in a pool with no provider of its own connected
  // shows as a provider too, so every account has a provider over it.
  const poolOnlyFamilies = $derived.by(() => {
    if (!router) return []
    const covered = new Set(shownProviders.map((entry) => entry.family).filter(Boolean))
    const needle = query.trim().toLowerCase()
    return [...new Set(router.accounts.map((account) => account.family))]
      .filter((family) => !covered.has(family))
      .filter((family) => !needle || familyName(family).toLowerCase().includes(needle))
  })

  $effect(() => { void load() })
  $effect(() => { void loadRouter() })

  async function loadRouter() {
    try {
      router = await tauri.invoke('model_router_settings')
    } catch (_) {
      router = null
    }
  }

  // Every router command answers with the whole settings, so one reply
  // redraws every card, and the inventory rereads so the picker follows.
  function onRouterSettings(next) {
    router = next
    void load()
  }
  onDestroy(() => { stopListening() })

  async function load() {
    try {
      inventory = await tauri.invoke('local_mode_provider_inventory')
      loadError = ''
      oninventory?.(inventory)
    } catch (_) {
      loadError = 'Muniment cannot read provider settings. Restart the app to retry.'
    }
  }

  function isDefault(entry, model) {
    return inventory?.default_provider === entry.id && inventory?.default_model === model.id
  }

  // A pick shows at once. The save follows, and a failed save puts the list back.
  async function setDefault(entry, model) {
    const before = inventory
    inventory = { ...inventory, default_provider: entry.id, default_model: model.id }
    oninventory?.(inventory)
    status = `${model.id} is the default model.`
    try {
      await tauri.invoke('local_mode_set_default_model', { provider: entry.id, model: model.id })
    } catch (error) {
      inventory = before
      oninventory?.(inventory)
      status = String(error?.message ?? error)
    }
  }

  async function toggleHidden(entry, model) {
    const key = modelKey(entry.id, model.id)
    const before = inventory
    const shown = !hidden.has(key)
    inventory = { ...inventory, hidden: shown ? [...(inventory.hidden ?? []), key] : (inventory.hidden ?? []).filter((entry) => entry !== key) }
    oninventory?.(inventory)
    try {
      await tauri.invoke('local_mode_set_model_hidden', { provider: entry.id, model: model.id, hidden: shown })
    } catch (error) {
      inventory = before
      oninventory?.(inventory)
      status = String(error?.message ?? error)
    }
  }

  async function disconnect(entry) {
    try {
      await tauri.invoke('local_mode_disconnect_provider', { provider: entry.id })
      status = `${entry.name} is disconnected.`
      await load()
    } catch (error) {
      status = String(error?.message ?? error)
    }
  }

  function openConnector() {
    providerQuery = ''
    view = 'connect'
  }

  function chooseProvider(id) {
    origin = view
    providerId = id
    formError = ''
    key = ''
    endpointName = ''
    endpointModels = ''
    baseUrl = catalogProvider(id)?.baseUrl ?? ''
    chooseMethod(catalogProvider(id)?.methods?.[0] ?? 'key')
  }

  function chooseMethod(next) {
    method = next
    formError = ''
    login = null
    view = 'method'
    if (next === 'claude-code') void loadClaudeStatus()
  }

  function back() {
    if (login?.stage && login.stage !== 'done' && login.stage !== 'failed' && login.stage !== 'cancelled') void cancelLogin()
    login = null
    formError = ''
    view = view === 'method' ? origin : 'list'
  }

  async function finishConnect(message) {
    status = message
    await load()
    view = 'list'
    login = null
  }

  async function saveKey() {
    if (pending || !key.trim()) return
    pending = true
    formError = ''
    try {
      await tauri.invoke('local_mode_store_provider_key', { provider: providerId, key })
      key = ''
      await finishConnect(`Muniment saved the ${provider.name} key.`)
    } catch (error) {
      formError = String(error?.message ?? error) || 'Muniment could not save the provider key. Try again.'
    } finally {
      pending = false
    }
  }

  async function saveOllama() {
    if (pending || !baseUrl.trim()) return
    pending = true
    formError = ''
    try {
      await tauri.invoke('local_mode_store_local_provider', { baseUrl })
      await finishConnect('Muniment saved the Ollama server.')
    } catch (error) {
      formError = String(error?.message ?? error) || 'Ollama setup failed. Check the URL, then retry.'
    } finally {
      pending = false
    }
  }

  async function saveEndpoint() {
    const models = endpointModels.split(/[\n,]/).map((model) => model.trim()).filter(Boolean)
    if (pending || !baseUrl.trim() || (providerId === 'custom' && !endpointName.trim())) return
    pending = true
    formError = ''
    try {
      await tauri.invoke('local_mode_store_endpoint', {
        kind: providerId === 'lmstudio' ? 'lmstudio' : 'custom',
        name: providerId === 'lmstudio' ? 'LM Studio' : endpointName.trim(),
        baseUrl,
        key,
        models,
      })
      key = ''
      await finishConnect(`Muniment saved the ${providerId === 'lmstudio' ? 'LM Studio' : endpointName.trim()} endpoint.`)
    } catch (error) {
      formError = String(error?.message ?? error) || 'Muniment could not save the endpoint. Check the URL, then retry.'
    } finally {
      pending = false
    }
  }

  async function loadClaudeStatus() {
    claude = null
    try {
      claude = await tauri.invoke('local_mode_claude_code_status')
    } catch (_) {
      claude = { installed: false, logged_in: false, path: null }
    }
  }

  async function connectClaudeCode() {
    if (pending || !claude?.path) return
    pending = true
    formError = ''
    try {
      await tauri.invoke('local_mode_connect_claude_code', { executable: claude.path })
      await finishConnect('Anthropic is connected through Claude Code. Its models appear after the next message.')
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

  async function startLogin() {
    const account = provider?.account
    if (!account || pending) return
    pending = true
    formError = ''
    loginAnswer = ''
    login = { provider: account.provider, stage: 'start', message: 'Starting the sign-in…', url: '', code: '', prompt: null }
    stopListening()
    unlisten = listen('local-mode-login', ({ payload }) => onLoginEvent(payload))
    try {
      await tauri.invoke('local_mode_account_login_start', { provider: account.provider })
    } catch (error) {
      login = { ...login, stage: 'failed', message: String(error?.message ?? error) }
      stopListening()
    } finally {
      pending = false
    }
  }

  function onLoginEvent(payload) {
    if (!login || (payload.provider && payload.provider !== login.provider && payload.stage !== 'cancelled')) return
    switch (payload.stage) {
      case 'event': {
        const event = payload.event ?? {}
        if (event.type === 'auth_url') {
          login = { ...login, stage: 'browser', url: event.url, message: event.instructions ?? 'Continue in your browser.' }
          void tauri.invoke('local_mode_open_url', { url: event.url }).catch(() => {})
        } else if (event.type === 'device_code') {
          login = { ...login, stage: 'code', url: event.verificationUri ?? event.verificationUriComplete ?? '', code: event.userCode ?? '', message: 'Enter this code on the sign-in page.' }
        } else if (event.message) {
          login = { ...login, message: event.message }
        }
        break
      }
      case 'prompt':
        login = { ...login, prompt: payload }
        break
      case 'done':
        stopListening()
        void finishConnect(`${provider?.name ?? 'The provider'} account is connected.`)
        break
      case 'failed':
        stopListening()
        login = { ...login, stage: 'failed', message: payload.message || 'The sign-in did not finish. Try again.', prompt: null }
        break
      case 'cancelled':
        stopListening()
        login = { ...login, stage: 'cancelled', message: 'The sign-in was cancelled.', prompt: null }
        break
      case 'exit':
        if (login.stage !== 'done' && login.stage !== 'failed' && login.stage !== 'cancelled') {
          stopListening()
          login = { ...login, stage: 'failed', message: 'The sign-in stopped before it finished. Try again.', prompt: null }
        }
        break
      default:
        if (payload.message) login = { ...login, message: payload.message }
    }
  }

  async function answerPrompt(value) {
    const prompt = login?.prompt
    if (!prompt) return
    try {
      await tauri.invoke('local_mode_account_login_answer', { id: prompt.id, value, confirmed: null })
      login = { ...login, prompt: null }
      loginAnswer = ''
    } catch (error) {
      formError = String(error?.message ?? error)
    }
  }

  async function cancelLogin() {
    try { await tauri.invoke('local_mode_account_login_cancel') } catch (_) {}
  }

  function openUrl(url) {
    void tauri.invoke('local_mode_open_url', { url }).catch(() => { formError = 'The browser could not open. Copy the link instead.' })
  }
</script>

<div class="models">
  {#if view === 'list'}
    <header class="models-head">
      <div>
        <h4 class="models-label">Providers</h4>
        <p class="support">Every connection is an account under its provider, a key or a subscription. Pick which models the selector shows.</p>
      </div>
      <button type="button" class="connect" onclick={openConnector}><LucideIcon name="plus" variant="action" size={14} />Connect provider</button>
    </header>
    {#if loadError}<p class="support" role="alert">{loadError}</p>{/if}
    {#if status}<p class="support" role="status">{status}</p>{/if}
    <div class="search">
      <LucideIcon name="search" variant="action" size={14} />
      <input type="search" aria-label="Search models" placeholder="Search models" bind:value={query}>
    </div>
    {#if inventory && inventory.providers.length === 0}
      <p class="support empty">No provider is connected. Pick one below to connect it.</p>
    {/if}
    {#each shownProviders as entry (entry.id)}
      <section class="provider-group" aria-label={entry.name}>
        <header>
          <ProviderLogo provider={entry.id} size={18} />
          <h5>{entry.name}</h5>
          <button type="button" class="quiet disconnect" onclick={() => disconnect(entry)}>Disconnect</button>
        </header>
        <ul class="account-list">
          <li class="account-row">
            <span class="account-name">{sourceTag(entry.source) === 'Account' ? 'Signed-in account' : `${sourceTag(entry.source)} connection`}</span>
            <span class="tag">{sourceTag(entry.source)}</span>
            {#if entry.base_url}<span class="record">{entry.base_url}</span>{/if}
            <span class="record">direct</span>
          </li>
        </ul>
        {#if entry.family && router}
          <ModelAccounts {tauri} {listen} settings={router} family={entry.family} onsettings={onRouterSettings} />
        {/if}
        {#if entry.models.length === 0}
          <p class="support">{entry.source === 'claude-code' ? 'Models appear after the next message installs the bridge.' : 'No models answer yet.'}</p>
        {:else}
          <ul class="model-list">
            {#each entry.models as model (model.id)}
              {@const shown = !hidden.has(modelKey(entry.id, model.id))}
              <li class="model-row">
                <span class="model-id">{model.id}</span>
                <span class="record">{model.context}</span>
                {#if isDefault(entry, model)}
                  <span class="tag default">Default</span>
                {:else}
                  <button type="button" class="quiet use" disabled={!shown} onclick={() => setDefault(entry, model)}>Use</button>
                {/if}
                <button type="button" role="switch" class="switch" aria-checked={shown} aria-label={`Show ${model.id} in the selector`} onclick={() => toggleHidden(entry, model)}><span></span></button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/each}
    {#each poolOnlyFamilies as family (family)}
      <section class="provider-group" aria-label={familyName(family)}>
        <header>
          <ProviderLogo provider={familyProvider(family)} size={18} />
          <h5>{familyName(family)}</h5>
          <span class="tag">Pool</span>
        </header>
        <ModelAccounts {tauri} {listen} settings={router} {family} onsettings={onRouterSettings} />
      </section>
    {/each}
    {#if inventory && connectable.length}
      <section class="provider-group connectable" aria-label="Connect">
        <h5 class="group-label">Connect</h5>
        <ul class="provider-list">
          {#each connectable as entry (entry.id)}
            <li><button type="button" class="quiet provider-row" onclick={() => chooseProvider(entry.id)}><ProviderLogo provider={entry.id} size={18} /><span>{entry.name}</span><span class="tag">{entry.methods.map((m) => methodLabel(entry, m)).join(' · ')}</span></button></li>
          {/each}
        </ul>
      </section>
    {/if}
    <ModelRouterSection {tauri} settings={router} onsettings={onRouterSettings} />
  {:else}
    <header class="connect-head">
      <button type="button" class="quiet back" aria-label="Back" onclick={back}><LucideIcon name="arrow-left" variant="action" size={16} /></button>
      {#if view === 'connect'}
        <h4>Connect provider</h4>
      {:else}
        <ProviderLogo provider={providerId} size={18} />
        <h4>Connect {provider?.name}</h4>
      {/if}
    </header>
    {#if view === 'connect'}
      <div class="search">
        <LucideIcon name="search" variant="action" size={14} />
        <input type="search" aria-label="Search providers" placeholder="Search providers" bind:value={providerQuery}>
      </div>
      {#each [['Popular', catalog.popular], ['Other', catalog.other]] as [group, entries]}
        {#if entries.length}
          <h5 class="group-label">{group}</h5>
          <ul class="provider-list">
            {#each entries as entry (entry.id)}
              <li><button type="button" class="quiet provider-row" onclick={() => chooseProvider(entry.id)}><ProviderLogo provider={entry.id} size={18} /><span>{entry.name}</span><span class="tag">{entry.methods.map((m) => methodLabel(entry, m)).join(' · ')}</span></button></li>
            {/each}
          </ul>
        {/if}
      {/each}
    {:else if method === 'key'}
      <p class="support">Enter your {provider.name} API key. It goes into the harness credential store on this device.</p>
      <label for="provider-key">{provider.name} API key</label>
      <input id="provider-key" type="password" autocomplete="off" bind:value={key} disabled={pending}>
      {#if formError}<p class="support" role="alert">{formError}</p>{/if}
      <button type="button" disabled={pending || !key.trim()} onclick={saveKey}>Save key</button>
    {:else if method === 'ollama'}
      <p class="support">Point Muniment at a running Ollama server. It asks the server for every model it serves.</p>
      <label for="provider-base-url">Ollama server URL</label>
      <input id="provider-base-url" type="url" placeholder="http://localhost:11434/v1" autocomplete="url" bind:value={baseUrl} disabled={pending}>
      {#if formError}<p class="support" role="alert">{formError}</p>{/if}
      <button type="button" disabled={pending || !baseUrl.trim()} onclick={saveOllama}>Save Ollama server</button>
    {:else if method === 'endpoint'}
      <p class="support">Any OpenAI-compatible server: {provider.id === 'lmstudio' ? 'LM Studio on this device.' : 'a LiteLLM proxy or another gateway.'}</p>
      {#if provider.id === 'custom'}
        <label for="endpoint-name">Name</label>
        <input id="endpoint-name" type="text" bind:value={endpointName} disabled={pending}>
      {/if}
      <label for="provider-base-url">Server URL</label>
      <input id="provider-base-url" type="url" placeholder={provider.baseUrl ?? 'http://localhost:4000/v1'} autocomplete="url" bind:value={baseUrl} disabled={pending}>
      <label for="endpoint-key">API key (optional)</label>
      <input id="endpoint-key" type="password" autocomplete="off" bind:value={key} disabled={pending}>
      <label for="endpoint-models">Models, one per line</label>
      <textarea id="endpoint-models" rows="3" bind:value={endpointModels} disabled={pending}></textarea>
      <p class="support">Leave the list empty and Muniment asks the server for its models.</p>
      {#if formError}<p class="support" role="alert">{formError}</p>{/if}
      <button type="button" disabled={pending || !baseUrl.trim() || (provider.id === 'custom' && !endpointName.trim())} onclick={saveEndpoint}>Save endpoint</button>
    {:else if method === 'claude-code'}
      <p class="support">Anthropic through your Claude Code sign-in. Muniment installs the bridge package and routes Claude models through it.</p>
      {#if claude === null}
        <p class="support" role="status">Checking Claude Code…</p>
      {:else if !claude.installed}
        <p class="support" role="status">Claude Code is not installed on this device. Install it, run <code>claude</code> once to sign in, then retry.</p>
        <button type="button" onclick={loadClaudeStatus}>Check again</button>
      {:else if !claude.logged_in}
        <p class="support" role="status">Claude Code is installed but not signed in. Run <code>claude</code> in a terminal and sign in, then retry.</p>
        <button type="button" onclick={loadClaudeStatus}>Check again</button>
      {:else}
        <p class="support" role="status">Claude Code is signed in.</p>
        {#if formError}<p class="support" role="alert">{formError}</p>{/if}
        <button type="button" disabled={pending} onclick={connectClaudeCode}>Connect Claude Code</button>
      {/if}
    {:else if method === 'account'}
      <p class="support">Sign in with your {methodLabel(provider, 'account')}. The browser opens, and the token stays on this device.</p>
      {#if !login}
        <button type="button" disabled={pending} onclick={startLogin}>Sign in</button>
      {:else}
        <div class="login" aria-live="polite">
          <p class="support">{login.message}</p>
          {#if login.code}<p class="login-code">{login.code}</p>{/if}
          {#if login.url}
            <button type="button" onclick={() => openUrl(login.url)}>Open the sign-in page</button>
            <p class="record login-url">{login.url}</p>
          {/if}
          {#if login.prompt?.kind === 'select'}
            <p class="support">{login.prompt.title}</p>
            <div class="login-options">
              {#each login.prompt.options as option}
                <button type="button" onclick={() => answerPrompt(option)}>{option}</button>
              {/each}
            </div>
          {:else if login.prompt?.kind === 'input'}
            <label for="login-answer">{login.prompt.title}</label>
            <input id="login-answer" type="text" placeholder={login.prompt.placeholder ?? ''} bind:value={loginAnswer}>
            <button type="button" disabled={!loginAnswer.trim()} onclick={() => answerPrompt(loginAnswer.trim())}>Continue</button>
          {/if}
          {#if login.stage === 'failed' || login.stage === 'cancelled'}
            <button type="button" onclick={startLogin}>Try again</button>
          {:else}
            <button type="button" class="quiet" onclick={cancelLogin}>Cancel</button>
          {/if}
        </div>
      {/if}
      {#if formError}<p class="support" role="alert">{formError}</p>{/if}
    {/if}
    {#if view === 'method' && (provider?.methods.length ?? 0) > 1}
      <p class="method-switch">
        {#each provider.methods.filter((entry) => entry !== method) as entry (entry)}
          <button type="button" class="quiet" onclick={() => chooseMethod(entry)}>{entry === 'key' ? 'Use an API key instead' : entry === 'account' ? `Sign in with your ${methodLabel(provider, 'account')} instead` : `Use ${methodLabel(provider, entry)} instead`}</button>
        {/each}
      </p>
    {/if}
  {/if}
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .models { display: grid; gap: 12px; align-content: start; }
  .account-list { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .account-row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-height: 28px; padding: 3px 4px; }
  .account-name { font-size: var(--text-13); }
  .models-head, .connect-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .connect-head { justify-content: flex-start; }
  .connect-head h4, .models-head h4 { margin: 0; }
  .models-label { color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .connect-head h4 { font-size: var(--text-15); font-weight: 600; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .support code { font: var(--text-12) var(--font-mono); }
  .empty { padding: 12px 0; }
  .connect { display: inline-flex; align-items: center; gap: 6px; min-height: 28px; padding: 4px 10px; white-space: nowrap; }
  .back { min-width: 28px; min-height: 28px; padding: 5px; line-height: 0; }
  .search { display: flex; align-items: center; gap: 8px; padding: 7px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); }
  .search:focus-within { border-color: var(--muted); }
  .search input { flex: 1; min-width: 0; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; font-size: var(--text-13); }
  .provider-group { display: grid; gap: 4px; padding: 10px 0 6px; border-top: 1px solid var(--border); }
  .provider-group header { display: flex; align-items: center; gap: 8px; min-height: 28px; }
  .provider-group h5 { margin: 0; font-size: var(--text-13); font-weight: 600; }
  .tag, .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .disconnect { margin-left: auto; min-height: 24px; padding: 2px 8px; font-size: var(--text-12); }
  .model-list, .provider-list { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .model-row { display: flex; align-items: center; gap: 10px; min-height: 32px; padding: 3px 4px; }
  .model-id { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font: var(--text-12) var(--font-mono); }
  .use { min-height: 24px; padding: 2px 8px; font-size: var(--text-12); }
  .default { color: var(--ink); }
  /* The show switch: a hairline track and a muted knob when the model is hidden, a signal track and knob at the right when it shows. §1.2 lists the switch. */
  .switch { position: relative; flex: none; width: 30px; height: 18px; padding: 0; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--paper); }
  .switch span { position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: var(--radius-chip); background: var(--muted); transition: transform 120ms ease, background 120ms ease; }
  .switch[aria-checked="true"] { border-color: var(--signal); background: var(--signal-soft); }
  .switch[aria-checked="true"] span { transform: translateX(12px); background: var(--signal); }
  .switch:hover:not(:disabled) { background: var(--faint); }
  .group-label { margin: 6px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .provider-row { display: flex; width: 100%; align-items: center; gap: 10px; min-height: 36px; padding: 6px 8px; text-align: left; font-size: var(--text-13); }
  .provider-row .tag { margin-left: auto; text-align: right; }
  label { color: var(--muted); font: var(--text-12) var(--font-mono); }
  input[type="password"], input[type="url"], input[type="text"], textarea { width: min(420px, 100%); box-sizing: border-box; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: inherit; font-size: var(--text-13); }
  input:focus, textarea:focus { border-color: var(--muted); outline: 0; }
  textarea { resize: vertical; font-family: var(--font-mono); font-size: var(--text-12); }
  .models > button, .login > button { justify-self: start; min-height: 28px; padding: 4px 10px; }
  .login { display: grid; gap: 8px; justify-items: start; }
  .login-code { margin: 0; font: var(--text-22) var(--font-mono); letter-spacing: .08em; }
  .login-url { max-width: 100%; }
  .login-options { display: flex; flex-wrap: wrap; gap: 6px; }
  .connectable { margin-top: 4px; }
  .connectable .group-label { margin: 0 0 4px; }
  .method-switch { display: flex; flex-wrap: wrap; gap: 6px; margin: 4px 0 0; }
  .method-switch button { min-height: 24px; padding: 2px 8px; color: var(--muted); font-size: var(--text-12); }
  .method-switch button:hover:not(:disabled) { color: var(--ink); }
</style>

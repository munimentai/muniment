<script>
  // Settings → Models → Router: the account pools behind one provider, what
  // each account has served, the routes, and the classifier that picks one.
  import LucideIcon from './LucideIcon.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import { catalog, matchSaved, priceLabel } from './classifier-catalog.js'
  import { connectedFamilies, familyModels, routeReady, suggestRoutes, suggestedFallback } from './route-catalog.js'

  let { tauri } = $props()

  let settings = $state(null)
  let loadError = $state('')
  let status = $state('')
  let formError = $state('')
  let pending = $state(false)
  let view = $state('accounts')
  // The provider filter over the account cards: `all` or one family id.
  let pool = $state('all')

  // The add-account form.
  let family = $state('openai')
  let label = $state('')
  let key = $state('')
  let baseUrl = $state('')
  let accountModels = $state('')

  // The classifier form. The key is write-only: Settings never reads one back.
  let classifierKind = $state('none')
  let classifierFamily = $state('')
  let classifierKey = $state('')
  let classifierModel = $state('jev-latest')
  let classifierUrl = $state('')
  let classifierStatus = $state('')
  // The catalog row in hand, `endpoint` for a server of the user's own, and
  // the empty string for no classifier at all.
  let chosen = $state('')

  // The routes table, edited as a whole and saved in one call.
  let routes = $state([])
  let fallback = $state('')
  let confidence = $state(0.55)

  const families = $derived(settings?.families ?? [])
  // One summary per provider that holds an account: what its pool has served,
  // how much of it is in flight, and how many accounts carry it.
  const pools = $derived.by(() => {
    if (!settings) return []
    return families
      .map((entry) => {
        const accounts = settings.accounts.filter((account) => account.family === entry.id)
        const today = accounts.reduce((sum, account) => sum + (account.days.at(-1)?.[1] ?? 0), 0)
        return {
          ...entry,
          accounts,
          requests: accounts.reduce((sum, account) => sum + account.requests, 0),
          today,
          errors: accounts.reduce((sum, account) => sum + account.errors, 0),
          active: accounts.reduce((sum, account) => sum + account.active, 0),
          live: accounts.filter((account) => account.enabled && !cooling(account)).length,
        }
      })
      .filter((entry) => entry.accounts.length > 0)
  })
  const shown = $derived(pool === 'all' ? pools : pools.filter((entry) => entry.id === pool))
  const busiestPool = $derived(Math.max(1, ...pools.map((entry) => entry.requests)))
  const rows = $derived(catalog(settings?.accounts ?? []))
  const connected = $derived(connectedFamilies(settings?.accounts ?? []))
  const busiest = $derived(Math.max(1, ...(settings?.accounts ?? []).flatMap((account) => account.days.map((day) => day[1]))))

  $effect(() => { void load() })

  async function load() {
    try {
      apply(await tauri.invoke('model_router_settings'))
      loadError = ''
    } catch (error) {
      loadError = String(error?.message ?? error) || 'Muniment cannot read the router settings.'
    }
  }

  // Every command answers with the whole settings, so one reply redraws the page.
  function apply(next) {
    settings = next
    routes = next.routes.map((route) => ({ ...route }))
    fallback = next.fallback ?? ''
    confidence = next.min_confidence
    classifierKind = next.classifier.kind
    classifierFamily = next.classifier.family ?? ''
    classifierModel = next.classifier.model || 'jev-latest'
    classifierUrl = next.classifier.base_url ?? ''
    chosen = matchSaved(next.classifier)
  }

  // Picking a catalog row fills the form. Saving is still its own step, so a
  // key or a URL can be typed first.
  function choose(row) {
    chosen = row.id
    classifierKind = row.kind
    classifierModel = row.model
    classifierFamily = row.family ?? ''
    classifierStatus = ''
  }

  function chooseEndpoint() {
    chosen = 'endpoint'
    classifierKind = 'endpoint'
    classifierStatus = ''
  }

  function chooseNone() {
    chosen = ''
    classifierKind = 'none'
    classifierStatus = ''
  }

  async function run(command, args, done) {
    if (pending) return
    pending = true
    formError = ''
    try {
      apply(await tauri.invoke(command, args))
      if (done) status = done
    } catch (error) {
      formError = String(error?.message ?? error)
    } finally {
      pending = false
    }
  }

  function toggleRouter() {
    void run('model_router_set_enabled', { enabled: !settings.enabled }, settings.enabled ? 'The router is off.' : 'The router is on.')
  }

  function addAccount() {
    const models = accountModels.split(/[\n,]/).map((model) => model.trim()).filter(Boolean)
    void run('model_router_add_account', { family, label, key, baseUrl: baseUrl.trim() || null, models }, `${label} is in the ${family} pool.`)
      .then(() => { if (!formError) { label = ''; key = ''; baseUrl = ''; accountModels = '' } })
  }

  function setEnabled(account, enabled) {
    void run('model_router_update_account', { id: account.id, enabled })
  }

  function setWeight(account, weight) {
    const value = Number(weight)
    if (!Number.isFinite(value) || value < 0) return
    void run('model_router_update_account', { id: account.id, weight: Math.round(value) })
  }

  function removeAccount(account) {
    void run('model_router_remove_account', { id: account.id }, `${account.label} is removed.`)
  }

  function addRoute() {
    const first = connected[0] ?? families[0]?.id ?? 'openai'
    routes = [...routes, { key: '', description: '', family: first, model: familyModels(first)[0]?.model ?? '' }]
  }

  // The routes the pools as they stand can serve. It replaces the table, and
  // saving is still its own step, so nothing is lost without a click.
  function suggest() {
    const next = suggestRoutes(settings?.accounts ?? [])
    if (next.length === 0) return
    routes = next
    fallback = suggestedFallback(next)
    status = 'Routes suggested from your pools. Save them to keep them.'
  }

  // Changing a route's provider moves its model to that provider's own.
  function setRouteFamily(route, family) {
    route.family = family
    if (!familyModels(family).some((entry) => entry.model === route.model)) {
      route.model = familyModels(family)[0]?.model ?? ''
    }
    routes = [...routes]
  }

  function removeRoute(index) {
    routes = routes.filter((_, at) => at !== index)
    if (!routes.some((route) => route.key === fallback)) fallback = routes[0]?.key ?? ''
  }

  function saveRoutes() {
    void run('model_router_save_routes', { routes: routes.map((route) => ({ ...route })), fallback: fallback || null, minConfidence: Number(confidence) }, 'The routes are saved.')
  }

  function saveClassifier() {
    void run('model_router_set_classifier', {
      kind: classifierKind,
      apiKey: classifierKey,
      model: classifierModel,
      baseUrl: classifierUrl.trim() || null,
      family: classifierFamily || null,
    }, 'The classifier is saved.').then(() => { if (!formError) classifierKey = '' })
  }

  async function testClassifier() {
    classifierStatus = 'Asking the classifier…'
    try {
      await tauri.invoke('model_router_test_classifier')
      classifierStatus = 'The classifier answered.'
    } catch (error) {
      classifierStatus = String(error?.message ?? error)
    }
  }

  function tokens(count) {
    if (count < 1000) return String(count)
    if (count < 1_000_000) return `${(count / 1000).toFixed(1)}K`
    return `${(count / 1_000_000).toFixed(2)}M`
  }

  function when(ms) {
    if (!ms) return 'never'
    const seconds = Math.round((Date.now() - ms) / 1000)
    if (seconds < 60) return 'just now'
    if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`
    if (seconds < 86400) return `${Math.round(seconds / 3600)}h ago`
    return `${Math.round(seconds / 86400)}d ago`
  }

  function cooling(account) {
    return account.cooldown_until_ms && account.cooldown_until_ms > Date.now()
  }
</script>

<div class="router">
  <header class="router-head">
    <div>
      <h4 class="router-label">Model router</h4>
      <p class="support">Many accounts per provider, balanced across them. Off leaves every turn on the provider you picked.</p>
    </div>
    {#if settings}
      <button type="button" role="switch" class="switch" aria-checked={settings.enabled} aria-label="Turn the model router on" onclick={toggleRouter} disabled={pending}><span></span></button>
    {/if}
  </header>

  {#if loadError}<p class="support" role="alert">{loadError}</p>{/if}
  {#if status}<p class="support" role="status">{status}</p>{/if}
  {#if formError}<p class="support" role="alert">{formError}</p>{/if}

  {#if settings?.enabled}
    <p class="record endpoint">
      {settings.base_url ?? 'starting…'}
      {#if !settings.is_default}<span class="tag warn">Pick a router model in the model chip to send turns here.</span>{/if}
    </p>

    <nav class="tabs" aria-label="Router settings">
      {#each [['accounts', 'Accounts'], ['routes', 'Routes'], ['classifier', 'Classifier']] as [id, name]}
        <button type="button" class="quiet tab" aria-current={view === id ? 'true' : undefined} onclick={() => { view = id }}>{name}</button>
      {/each}
    </nav>

    {#if view === 'accounts'}
      {#if pools.length === 0}
        <p class="support empty">No account is in a pool. Add one below, then add a second of the same provider to balance them.</p>
      {:else}
        <ul class="summaries">
          {#each pools as entry (entry.id)}
            <li class="summary">
              <header>
                <ProviderLogo provider={entry.id} size={18} />
                <h5>{entry.name}</h5>
                <span class="tag">{entry.accounts.length} {entry.accounts.length === 1 ? 'account' : 'accounts'}</span>
              </header>
              <p class="headline">{entry.requests}<span class="unit">turns · {entry.live} of {entry.accounts.length} ready</span></p>
              <div class="share" aria-label={`${entry.name} share of the router's turns`}>
                <span style={`width: ${Math.round((entry.requests / busiestPool) * 100)}%`}></span>
              </div>
              <p class="record foot">{entry.active} active · {entry.today} today · {entry.errors} {entry.errors === 1 ? 'error' : 'errors'}</p>
            </li>
          {/each}
        </ul>

        <div class="filters">
          <button type="button" class="quiet chip" aria-pressed={pool === 'all'} onclick={() => { pool = 'all' }}>All pools</button>
          {#each pools as entry (entry.id)}
            <button type="button" class="quiet chip" aria-pressed={pool === entry.id} onclick={() => { pool = entry.id }}>{entry.name}</button>
          {/each}
          <span class="record filters-note">Turns served</span>
        </div>

        <ul class="cards">
          {#each shown as entry (entry.id)}
            {#each entry.accounts as account (account.id)}
              <li class="card" class:cooling={cooling(account)}>
                <header>
                  <span class="card-label">{account.label}</span>
                  <ProviderLogo provider={account.family} size={16} />
                </header>
                <p class="record tier">{account.source === 'key' ? 'API key' : 'Account'}{#if account.models.length} · {account.models.join(' · ')}{/if}</p>
                {#if cooling(account)}<p class="tag warn">Rate limited · back in {Math.max(1, Math.round((account.cooldown_until_ms - Date.now()) / 1000))}s</p>{/if}
                <div class="bars" aria-label={`${account.label} turns per day`}>
                  {#each account.days as [day, requests] (day)}
                    <span class="bar" style={`height: ${Math.max(2, Math.round((requests / busiest) * 22))}px`} title={`${day}: ${requests} turns`}></span>
                  {/each}
                  {#if account.days.length === 0}<span class="record">No turn yet.</span>{/if}
                </div>
                <dl class="figures">
                  <div><dt>Turns</dt><dd>{account.requests}</dd></div>
                  <div><dt>Tokens</dt><dd>{tokens(account.input_tokens)} in · {tokens(account.output_tokens)} out</dd></div>
                  <div><dt>Active</dt><dd>{account.active}</dd></div>
                  <div><dt>Last used</dt><dd>{when(account.last_used_ms)}</dd></div>
                  <div><dt>Errors</dt><dd>{account.errors}</dd></div>
                  <div><dt>Share</dt><dd><input type="number" min="0" max="100" aria-label={`${account.label} share`} value={account.weight} onchange={(event) => setWeight(account, event.currentTarget.value)}></dd></div>
                </dl>
                {#if account.last_error}<p class="record error">{account.last_error}</p>{/if}
                <footer>
                  <button type="button" class="quiet remove" onclick={() => removeAccount(account)}>Remove</button>
                  <button type="button" role="switch" class="switch" aria-checked={account.enabled} aria-label={`Use ${account.label}`} onclick={() => setEnabled(account, !account.enabled)}><span></span></button>
                </footer>
              </li>
            {/each}
          {/each}
        </ul>
      {/if}

      <section class="pool add" aria-label="Add an account">
        <h5 class="group-label">Add an account</h5>
        <label for="router-family">Provider</label>
        <select id="router-family" bind:value={family} disabled={pending}>
          {#each families as entry (entry.id)}<option value={entry.id}>{entry.name}</option>{/each}
        </select>
        <label for="router-label">Name</label>
        <input id="router-label" type="text" placeholder="work" bind:value={label} disabled={pending}>
        <label for="router-key">API key</label>
        <input id="router-key" type="password" autocomplete="off" bind:value={key} disabled={pending}>
        <label for="router-base">Server URL (optional)</label>
        <input id="router-base" type="url" placeholder={families.find((entry) => entry.id === family)?.base_url ?? ''} bind:value={baseUrl} disabled={pending}>
        <label for="router-models">Models this account serves, one per line (optional)</label>
        <textarea id="router-models" rows="2" bind:value={accountModels} disabled={pending}></textarea>
        <p class="support">Leave the model list empty and the account serves every model of its provider.</p>
        <button type="button" disabled={pending || !label.trim() || !key.trim()} onclick={addAccount}>Add account</button>
      </section>

    {:else if view === 'routes'}
      <p class="support">A route is a model with a description. The classifier reads the descriptions and picks one per turn, so it only ever picks a model your pools already serve. With no classifier every turn takes the fallback.</p>
      {#if connected.length === 0}
        <p class="support empty">No provider is connected, so there is nothing to route between. Add an account first.</p>
      {/if}
      <ul class="route-list">
        {#each routes as route, index (index)}
          {@const ready = routeReady(route, settings?.accounts ?? [])}
          {@const known = familyModels(route.family)}
          <li class="route" class:unserved={!ready}>
            <input type="text" class="route-key" placeholder="fast" aria-label="Route name" bind:value={route.key}>
            <select aria-label="Route provider" value={route.family} onchange={(event) => setRouteFamily(route, event.currentTarget.value)}>
              {#each families as entry (entry.id)}<option value={entry.id}>{entry.name}{connected.includes(entry.id) ? '' : ' — no account'}</option>{/each}
            </select>
            <input type="text" class="route-model" list={`models-${route.family}`} placeholder="gpt-6-astra" aria-label="Route model" bind:value={route.model}>
            <datalist id={`models-${route.family}`}>
              {#each known as entry (entry.model)}<option value={entry.model}>{entry.name} · {priceLabel(entry.price)} · {entry.context}</option>{/each}
            </datalist>
            <input type="text" class="route-why" placeholder="A short question: a lookup, a one-line edit" aria-label="What this route is for" bind:value={route.description}>
            {#if !ready}<span class="tag warn">no account</span>{/if}
            <button type="button" class="quiet remove" aria-label={`Remove ${route.key || 'route'}`} onclick={() => removeRoute(index)}><LucideIcon name="x" variant="action" size={14} /></button>
          </li>
        {/each}
      </ul>
      <div class="route-actions">
        <button type="button" class="quiet add-route" onclick={addRoute}><LucideIcon name="plus" variant="action" size={14} />Add route</button>
        <button type="button" class="quiet add-route" disabled={connected.length === 0} onclick={suggest}>Suggest from your pools</button>
      </div>
      <label for="router-fallback">Fallback route</label>
      <select id="router-fallback" bind:value={fallback}>
        <option value="">The first route</option>
        {#each routes.filter((route) => route.key) as route (route.key)}<option value={route.key}>{route.key}</option>{/each}
      </select>
      <label for="router-confidence">Confidence floor · {Number(confidence).toFixed(2)}</label>
      <input id="router-confidence" type="range" min="0" max="1" step="0.05" bind:value={confidence}>
      <p class="support">A classification under the floor takes the fallback instead.</p>
      <button type="button" disabled={pending} onclick={saveRoutes}>Save routes</button>

    {:else}
      <p class="support">The classifier reads each turn and picks a route. It is optional. It sends the turn's last message to the service you pick, and a model on your own accounts spends that account.</p>
      {#each ['Built to classify', 'On your accounts'] as group}
        <h5 class="group-label">{group}</h5>
        <ul class="catalog">
          {#each rows.filter((row) => row.group === group) as row (row.id)}
            <li>
              <button type="button" class="quiet catalog-row" aria-pressed={chosen === row.id} disabled={!row.ready} onclick={() => choose(row)}>
                {#if row.family}<ProviderLogo provider={row.family} size={16} />{/if}
                <span class="catalog-name">{row.name}</span>
                <span class="record">{row.model}</span>
                <span class="tag">{priceLabel(row.price)}</span>
                {#if !row.ready}<span class="tag">no account</span>{/if}
                {#if chosen === row.id}<LucideIcon name="check" variant="action" size={14} />{/if}
              </button>
              {#if row.note && chosen === row.id}<p class="support note">{row.note}</p>{/if}
            </li>
          {/each}
        </ul>
      {/each}
      <h5 class="group-label">Your own</h5>
      <ul class="catalog">
        <li><button type="button" class="quiet catalog-row" aria-pressed={chosen === 'endpoint'} onclick={chooseEndpoint}><span class="catalog-name">Another endpoint</span><span class="record">answers the same choice question</span>{#if chosen === 'endpoint'}<LucideIcon name="check" variant="action" size={14} />{/if}</button></li>
        <li><button type="button" class="quiet catalog-row" aria-pressed={chosen === ''} onclick={chooseNone}><span class="catalog-name">No classifier</span><span class="record">every turn takes the fallback route</span>{#if chosen === ''}<LucideIcon name="check" variant="action" size={14} />{/if}</button></li>
      </ul>

      {#if classifierKind === 'endpoint'}
        <label for="classifier-url">Classifier URL</label>
        <input id="classifier-url" type="url" placeholder="https://host/v1/systemone" bind:value={classifierUrl} disabled={pending}>
      {/if}
      {#if classifierKind === 'typesafe' || classifierKind === 'endpoint'}
        <label for="classifier-key">{classifierKind === 'typesafe' ? 'TypeSafe API key' : 'API key (optional)'}</label>
        <input id="classifier-key" type="password" autocomplete="off" placeholder={settings.classifier.configured ? 'Saved. Type a new key to replace it.' : ''} bind:value={classifierKey} disabled={pending}>
      {/if}
      {#if classifierKind !== 'none'}
        <label for="classifier-model">Model</label>
        <input id="classifier-model" type="text" bind:value={classifierModel} disabled={pending}>
      {/if}
      <div class="classifier-actions">
        <button type="button" disabled={pending} onclick={saveClassifier}>Save classifier</button>
        {#if settings.classifier.configured && settings.classifier.kind !== 'pooled'}<button type="button" class="quiet" onclick={testClassifier}>Test</button>{/if}
      </div>
      {#if classifierStatus}<p class="support" role="status">{classifierStatus}</p>{/if}
    {/if}
  {/if}
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .router { display: grid; gap: 12px; align-content: start; }
  .router-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .router-head h4 { margin: 0; }
  .router-label { color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .empty { padding: 12px 0; }
  .tag, .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .warn { color: var(--signal); }
  .error { overflow-wrap: anywhere; }
  .endpoint { display: flex; flex-wrap: wrap; gap: 8px; margin: 0; }
  .tabs { display: flex; gap: 4px; border-bottom: 1px solid var(--border); }
  .tab { min-height: 28px; padding: 4px 10px; border-radius: 0; }
  .tab[aria-current="true"] { color: var(--ink); box-shadow: inset 0 -1px 0 var(--signal); }
  .pool { display: grid; gap: 6px; padding: 10px 0 6px; border-top: 1px solid var(--border); }
  .pool header { display: flex; align-items: center; gap: 8px; min-height: 28px; }
  .pool h5 { margin: 0; font-size: var(--text-13); font-weight: 600; }
  .route-list { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .remove { min-height: 24px; padding: 2px 8px; font-size: var(--text-12); }
  /* The three provider summaries: what each pool has served, and how much of
     it is moving right now. One card per provider that holds an account. */
  .summaries { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 16px; margin: 0; padding: 10px 0; border-top: 1px solid var(--border); list-style: none; }
  .summary { display: grid; gap: 6px; align-content: start; }
  .summary header { display: flex; align-items: center; gap: 8px; }
  .summary h5 { margin: 0; font-size: var(--text-13); font-weight: 600; }
  .summary .tag { margin-left: auto; }
  .headline { display: flex; align-items: baseline; gap: 8px; margin: 0; font-size: var(--text-22); }
  .unit { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .share { height: 4px; border-radius: 2px; background: var(--faint); }
  .share span { display: block; height: 100%; border-radius: 2px; background: var(--signal); }
  .foot { margin: 0; }
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .chip { min-height: 26px; padding: 3px 10px; border: 1px solid var(--border); border-radius: var(--radius-chip); font-size: var(--text-12); }
  .chip[aria-pressed="true"] { border-color: var(--signal); background: var(--signal-soft); }
  .filters-note { margin-left: auto; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .card { display: grid; gap: 6px; align-content: start; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .card.cooling { border-color: var(--signal); }
  .card header { display: flex; align-items: center; gap: 8px; }
  .card-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--text-13); }
  .tier { margin: 0; }
  .card footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding-top: 2px; }
  .figures { display: grid; grid-template-columns: 1fr 1fr; gap: 4px 10px; margin: 0; }
  .figures div { display: grid; gap: 1px; }
  .figures dt { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .figures dd { margin: 0; font: var(--text-12) var(--font-mono); }
  .figures input { width: 52px; }
  /* One bar per day the ledger keeps, so a glance shows which account carries the pool. */
  .bars { display: flex; align-items: flex-end; gap: 2px; min-height: 26px; }
  .bar { width: 5px; border-radius: 1px; background: var(--signal); opacity: .75; }
  .group-label { margin: 6px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .route { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .route-key { width: 96px; }
  .route-model { width: 168px; }
  .route-why { flex: 1; min-width: 160px; }
  .route.unserved .route-key { color: var(--muted); }
  .route-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .add-route { display: inline-flex; align-items: center; gap: 6px; justify-self: start; min-height: 28px; padding: 4px 10px; border: 1px solid var(--border); }
  .catalog { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .catalog-row { display: flex; width: 100%; align-items: center; gap: 10px; min-height: 34px; padding: 6px 8px; text-align: left; font-size: var(--text-13); }
  .catalog-row[aria-pressed="true"] { background: var(--faint); box-shadow: inset 2px 0 0 var(--signal); }
  .catalog-row:disabled .catalog-name { color: var(--muted); }
  .catalog-name { min-width: 140px; }
  .catalog-row .tag { margin-left: auto; }
  .catalog-row .tag + .tag, .catalog-row .tag + :global(svg) { margin-left: 0; }
  .note { padding: 0 8px 6px; }
  .classifier-actions { display: flex; gap: 6px; }
  label { color: var(--muted); font: var(--text-12) var(--font-mono); }
  input[type="password"], input[type="url"], input[type="text"], input[type="number"], select, textarea { box-sizing: border-box; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: inherit; font-size: var(--text-13); }
  input[type="password"], input[type="url"], select, textarea { width: min(420px, 100%); }
  input:focus, select:focus, textarea:focus { border-color: var(--muted); outline: 0; }
  input[type="range"] { width: min(300px, 100%); accent-color: var(--signal); }
  textarea { resize: vertical; font-family: var(--font-mono); font-size: var(--text-12); }
  .router > button, .pool > button { justify-self: start; min-height: 28px; padding: 4px 10px; }
  /* The same switch the provider list uses: a hairline track, the knob right and the signal track when on. */
  .switch { position: relative; flex: none; width: 30px; height: 18px; padding: 0; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--paper); }
  .switch span { position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: var(--radius-chip); background: var(--muted); transition: transform 120ms ease, background 120ms ease; }
  .switch[aria-checked="true"] { border-color: var(--signal); background: var(--signal-soft); }
  .switch[aria-checked="true"] span { transform: translateX(12px); background: var(--signal); }
  .switch:hover:not(:disabled) { background: var(--faint); }
</style>

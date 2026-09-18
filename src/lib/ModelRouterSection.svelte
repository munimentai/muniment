<script>
  // Routing, the foot of Settings → Models: the router switch, the classifier
  // that picks a model per turn, and the statements it reads for every model
  // in the running. The accounts themselves sit under their providers above.
  import LucideIcon from './LucideIcon.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import { catalog, matchSaved, priceLabel } from './classifier-catalog.js'

  let { tauri, settings = null, onsettings } = $props()

  let status = $state('')
  let formError = $state('')
  let pending = $state(false)

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

  // One draft per model in the running: its name and the statement the
  // classifier reads. Saving sends only the ones a user changed.
  let drafts = $state([])
  let fallback = $state('')
  let confidence = $state(0.6)
  let seen = null

  const rows = $derived(catalog(settings?.accounts ?? []))

  // Every command answers with the whole settings, so one reply redraws the page.
  $effect(() => {
    if (!settings || settings === seen) return
    seen = settings
    drafts = settings.options.map((option) => ({ ...option, draftKey: option.key, draftDescription: option.description }))
    fallback = settings.fallback ?? ''
    confidence = settings.min_confidence
    classifierKind = settings.classifier.kind
    classifierFamily = settings.classifier.family ?? ''
    classifierModel = settings.classifier.model || 'jev-latest'
    classifierUrl = settings.classifier.base_url ?? ''
    chosen = matchSaved(settings.classifier)
  })

  async function run(command, payload, done = '') {
    pending = true
    formError = ''
    try {
      onsettings?.(await tauri.invoke(command, payload))
      status = done
    } catch (error) {
      formError = String(error?.message ?? error)
    } finally {
      pending = false
    }
  }

  function toggleRouter() {
    void run('model_router_set_enabled', { enabled: !settings.enabled }, settings.enabled ? 'The router is off.' : 'The router is on.')
  }

  // Picking a catalog row fills the form. Saving is still its own step, so a
  // pick never spends a key or an account before the user says so.
  function choose(row) {
    chosen = row.id
    classifierKind = row.kind
    classifierModel = row.model
    classifierFamily = row.family ?? ''
    classifierUrl = ''
  }

  function chooseEndpoint() {
    chosen = 'endpoint'
    classifierKind = 'endpoint'
    classifierFamily = ''
  }

  function chooseNone() {
    chosen = ''
    classifierKind = 'none'
    classifierFamily = ''
  }

  // A statement the user changed, or one they changed before, travels as a
  // route. An untouched model keeps the catalog's words, which the app updates.
  function edited(draft) {
    return draft.named || draft.draftKey !== draft.key || draft.draftDescription !== draft.description
  }

  function resetDraft(draft) {
    draft.draftKey = `${draft.family}/${draft.model}`
    draft.draftDescription = ''
    drafts = [...drafts]
  }

  function saveRoutes() {
    const routes = drafts.filter(edited).map((draft) => ({
      key: draft.draftKey.trim() || `${draft.family}/${draft.model}`,
      description: draft.draftDescription.trim(),
      family: draft.family,
      model: draft.model,
    }))
    void run('model_router_save_routes', { routes, fallback: fallback || null, minConfidence: Number(confidence) }, 'The statements are saved.')
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
</script>

<section class="routing" aria-labelledby="routing-title">
  <header class="routing-head">
    <div>
      <h4 id="routing-title" class="routing-label">Routing</h4>
      <p class="support">Balance each turn across the accounts of a provider, and let a classifier pick the model. Off leaves every turn on the model you picked.</p>
    </div>
    {#if settings}
      <button type="button" role="switch" class="switch" aria-checked={settings.enabled} aria-label="Turn routing on" onclick={toggleRouter} disabled={pending}><span></span></button>
    {/if}
  </header>
  {#if status}<p class="support" role="status">{status}</p>{/if}
  {#if formError}<p class="support" role="alert">{formError}</p>{/if}

  {#if settings?.enabled}
    <p class="record endpoint">
      {settings.base_url ?? 'starting…'}
      {#if !settings.is_default}<span class="tag warn">Pick a routed model in the model chip to send turns here.</span>{/if}
    </p>

    <h5 class="group-label">Classifier</h5>
    <p class="support">The classifier reads each turn and picks a model. It is optional. It sends the turn's last message to the service you pick, and a model on your own accounts spends that account.</p>
    {#each ['Built to classify', 'On your accounts'] as group}
      <h6 class="catalog-label">{group}</h6>
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
    <h6 class="catalog-label">Your own</h6>
    <ul class="catalog">
      <li><button type="button" class="quiet catalog-row" aria-pressed={chosen === 'endpoint'} onclick={chooseEndpoint}><span class="catalog-name">Another endpoint</span><span class="record">answers the same choice question</span>{#if chosen === 'endpoint'}<LucideIcon name="check" variant="action" size={14} />{/if}</button></li>
      <li><button type="button" class="quiet catalog-row" aria-pressed={chosen === ''} onclick={chooseNone}><span class="catalog-name">No classifier</span><span class="record">every turn takes the fallback model</span>{#if chosen === ''}<LucideIcon name="check" variant="action" size={14} />{/if}</button></li>
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
    <div class="actions">
      <button type="button" disabled={pending} onclick={saveClassifier}>Save classifier</button>
      {#if settings.classifier.configured && settings.classifier.kind !== 'pooled'}<button type="button" class="quiet" onclick={testClassifier}>Test</button>{/if}
    </div>
    {#if classifierStatus}<p class="support" role="status">{classifierStatus}</p>{/if}

    <h5 class="group-label">In the running</h5>
    <p class="support">Every model an account serves is in the running, and the classifier picks between them each turn. It reads these statements, not the model names, so a statement says what work the model wins and what should send a query elsewhere.</p>
    {#if drafts.length === 0}
      <p class="support empty">Nothing is in the running. Add an account under a provider, and its models enter at once.</p>
    {/if}
    <ul class="running">
      {#each drafts as draft (draft.family + '/' + draft.model)}
        <li class="option">
          <header>
            <ProviderLogo provider={draft.family} size={16} />
            <span class="option-name">{draft.name || draft.model}</span>
            <span class="record">{draft.model}</span>
            {#if draft.tier}<span class="tag">{draft.tier}</span>{/if}
            {#if draft.price}<span class="tag">${draft.price}/${draft.output} per M</span>{/if}
            {#if draft.context}<span class="tag">{draft.context}</span>{/if}
          </header>
          <label for={`key-${draft.family}-${draft.model}`}>Name the classifier answers with</label>
          <input id={`key-${draft.family}-${draft.model}`} type="text" class="option-key" bind:value={draft.draftKey}>
          <label for={`why-${draft.family}-${draft.model}`}>Statement</label>
          <textarea id={`why-${draft.family}-${draft.model}`} rows="3" bind:value={draft.draftDescription}></textarea>
          {#if edited(draft)}
            <p class="record"><button type="button" class="quiet reset" onclick={() => resetDraft(draft)}>Use the built-in statement</button></p>
          {/if}
        </li>
      {/each}
    </ul>
    {#if drafts.length}
      <label for="router-fallback">Fallback</label>
      <select id="router-fallback" bind:value={fallback}>
        <option value="">The cheapest model in the running</option>
        {#each drafts as draft (draft.family + '/' + draft.model)}<option value={draft.draftKey}>{draft.draftKey}</option>{/each}
      </select>
      <label for="router-confidence">Confidence floor · {Number(confidence).toFixed(2)}</label>
      <input id="router-confidence" type="range" min="0" max="1" step="0.05" bind:value={confidence}>
      <p class="support">A classification under the floor takes the fallback instead.</p>
      <button type="button" class="save" disabled={pending} onclick={saveRoutes}>Save statements</button>
    {/if}
  {/if}
</section>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .routing { display: grid; gap: 10px; align-content: start; padding-top: 12px; border-top: 1px solid var(--border); }
  .routing-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .routing-head h4 { margin: 0; }
  .routing-label { color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .empty { padding: 6px 0; }
  .tag, .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .warn { color: var(--ink); }
  .endpoint { display: flex; flex-wrap: wrap; gap: 8px; margin: 0; }
  .group-label { margin: 8px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .catalog-label { margin: 4px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .catalog { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .catalog-row { display: flex; width: 100%; align-items: center; gap: 10px; min-height: 34px; padding: 6px 8px; text-align: left; font-size: var(--text-13); }
  .catalog-row[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  .catalog-row:disabled .catalog-name { color: var(--muted); }
  .catalog-name { min-width: 140px; }
  .catalog-row .tag { margin-left: auto; }
  .catalog-row .tag + .tag, .catalog-row .tag + :global(svg) { margin-left: 0; }
  .note { padding: 0 8px 6px; }
  .actions { display: flex; gap: 6px; }
  .running { display: grid; gap: 10px; margin: 0; padding: 0; list-style: none; }
  .option { display: grid; gap: 4px; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .option header { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .option-name { font-size: var(--text-13); }
  .option-key { width: min(280px, 100%); }
  .option textarea { width: 100%; }
  .reset { padding: 0; font-size: var(--text-12); color: var(--muted); text-decoration: underline; }
  .save { justify-self: start; min-height: 28px; padding: 4px 10px; }
  label { color: var(--muted); font: var(--text-12) var(--font-mono); }
  input[type="password"], input[type="url"], input[type="text"], select, textarea { box-sizing: border-box; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: inherit; font-size: var(--text-13); }
  input[type="password"], input[type="url"], select, textarea { width: min(420px, 100%); }
  input:focus, select:focus, textarea:focus { border-color: var(--muted); outline: 0; }
  input[type="range"] { width: min(300px, 100%); accent-color: var(--ink); }
  textarea { resize: vertical; font-family: var(--font-mono); font-size: var(--text-12); }
  .switch { position: relative; flex: none; width: 30px; height: 18px; padding: 0; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--paper); }
  .switch span { position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: var(--radius-chip); background: var(--muted); transition: transform 120ms ease, background 120ms ease; }
  .switch[aria-checked="true"] { border-color: var(--signal); background: var(--signal-soft); }
  .switch[aria-checked="true"] span { transform: translateX(12px); background: var(--signal); }
  .switch:hover:not(:disabled) { background: var(--faint); }
</style>

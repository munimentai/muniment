<script>
  import Toggle from './Toggle.svelte'
  // Routing settings: the router switch, the classifier
  // that picks a model per turn, and the statements it reads for every model
  // in the running. The accounts themselves sit under their providers above.
  import { modelLabel } from './model-label.js'
  import LucideIcon from './LucideIcon.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import { pickerGroups, currentModel } from './provider-catalog.js'
  import { catalog, matchSaved, priceLabel } from './classifier-catalog.js'

  let { tauri, settings = null, onsettings, inventory, oninventory } = $props()

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

  const modelRows = $derived([...new Map(pickerGroups(inventory).flatMap((group) => group.models).filter((model) => model.id !== 'auto').map((model) => [`${model.provider}/${model.choice}`, model])).values()])
  const selected = $derived(currentModel(inventory))
  const automatic = $derived(selected?.model === 'auto' && selected?.provider === 'muniment-router')

  async function selectModel(provider, model) {
    pending = true
    formError = ''
    try {
      if (provider === 'muniment-router' && !settings.enabled) onsettings?.(await tauri.invoke('model_router_set_enabled', { enabled: true }))
      await tauri.invoke('local_mode_set_default_model', { provider, model })
      oninventory?.(await tauri.invoke('local_mode_provider_inventory'))
    } catch (error) { formError = String(error?.message ?? error) }
    finally { pending = false }
  }

  const rows = $derived(catalog())

  // Every command answers with the whole settings, so one reply redraws the page.
  $effect(() => {
    if (!settings) return
    const signature = JSON.stringify([settings.options, settings.routes, settings.fallback, settings.min_confidence, settings.classifier])
    if (signature === seen) return
    seen = signature
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
    const endpoint = classifierKind === 'endpoint' ? classifierUrl : ''
    if (chosen !== row.id) classifierKey = ''
    chosen = row.id
    classifierKind = row.kind
    classifierModel = row.model
    classifierFamily = row.family ?? ''
    classifierUrl = row.kind === 'endpoint' ? endpoint : ''
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

  function saveRoutes() {
    const routes = settings.routes ?? []
    void run('model_router_save_routes', { routes, fallback: fallback || null, minConfidence: Number(confidence) }, 'The routing rules are saved.')
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

{#snippet endpointFields()}
  <div class="connection-fields" role="group" aria-label="Classifier connection">
      <label for="classifier-model">Model ID</label>
      <input id="classifier-model" type="text" bind:value={classifierModel} disabled={pending}>
      <label for="classifier-url">Classifier URL</label>
      <input id="classifier-url" type="url" placeholder="https://host/v1/systemone" bind:value={classifierUrl} disabled={pending}>
      <label for="classifier-key">API key (optional)</label>
      <input id="classifier-key" type="password" autocomplete="off" placeholder={settings.classifier.kind === 'endpoint' && settings.classifier.model === classifierModel && settings.classifier.configured ? 'Saved. Type a new key to replace it.' : ''} bind:value={classifierKey} disabled={pending}>
  </div>
{/snippet}

<section class="routing" aria-labelledby="routing-title">
  <header class="routing-head"><div><h4 id="routing-title">Routing</h4></div></header>
  {#if status}<p class="support" role="status">{status}</p>{/if}
  {#if formError}<p class="support" role="alert">{formError}</p>{/if}
  <div class="selection" role="group" aria-label="Model selection">
    <button type="button" aria-pressed={automatic} disabled={pending || !settings.options.length} onclick={() => selectModel('muniment-router', 'auto')}><strong>Choose automatically</strong><span>{settings.classifier.kind === 'none' ? 'Uses the fallback until a classifier is set.' : 'Pick a model for each message.'}</span></button>
    <button type="button" aria-pressed={!automatic} disabled={pending || !modelRows.length} onclick={() => { if (automatic && modelRows[0]) void selectModel(modelRows[0].provider, modelRows[0].choice) }}><strong>Use a specific model</strong><span>Use the same model for every message.</span></button>
  </div>
  {#if !automatic && modelRows.length}
    <label for="selected-model">Selected model</label>
    <select id="selected-model" value={JSON.stringify([selected?.provider, selected?.model])} disabled={pending} onchange={(event) => { const [provider, model] = JSON.parse(event.currentTarget.value); void selectModel(provider, model) }}>
      {#each modelRows as model (model.provider + '/' + model.choice)}<option value={JSON.stringify([model.provider, model.choice])}>{model.label}</option>{/each}
    </select>
  {/if}
  <dl class="routing-summary" aria-label="Saved routing settings">
    <div><dt>Classifier</dt><dd>{settings.classifier.kind === 'none' ? 'No classifier' : settings.classifier.configured ? modelLabel(settings.classifier.model) : 'Connection required'}</dd></div>
    <div><dt>Eligible models</dt><dd>{settings.options.length}</dd></div>
    <div><dt>Fallback</dt><dd>{settings.options.length ? modelLabel(settings.fallback) || 'Lowest known price' : 'No eligible model'}</dd></div>
  </dl>
  {#if !settings.options.length}<p class="support">Connect an account to choose models automatically.</p>{/if}
  <details class="configure">
    <summary>Classifier and fallback</summary>
    <div class="disclosure-body">
    <p class="support">Your last message goes to the classifier to choose a model.</p>
    {#each ['Built to classify', 'Self-hosted'] as group}
      <h6 class="catalog-label">{group}</h6>
      <ul class="catalog">
        {#each rows.filter((row) => row.group === group) as row (row.id)}
          <li>
            <button type="button" class="quiet catalog-row" aria-pressed={chosen === row.id} disabled={!row.ready} onclick={() => choose(row)}>
              {#if row.family || row.kind === 'typesafe'}<ProviderLogo provider={row.family || 'typesafe'} size={16} />{/if}
              <span class="catalog-name">{row.name}</span>
              <span class="record">{modelLabel(row.model)}</span>
              <span class="tag">{row.kind === 'endpoint' ? 'your server' : priceLabel(row.price)}</span>
              {#if !row.ready}<span class="tag">no account</span>{/if}
              {#if chosen === row.id}<LucideIcon name="check" variant="action" size={14} />{/if}
            </button>
            {#if row.note && chosen === row.id}<p class="support note">{row.note}</p>{/if}
            {#if row.kind === 'endpoint' && chosen === row.id}{@render endpointFields()}{/if}
            {#if row.kind === 'typesafe' && chosen === row.id}
              <label for="classifier-key">TypeSafe API key</label>
              <input id="classifier-key" type="password" autocomplete="off" placeholder={settings.classifier.kind === 'typesafe' && settings.classifier.configured ? 'Saved. Type a new key to replace it.' : ''} bind:value={classifierKey} disabled={pending}>
              <label for="classifier-model">Model ID</label>
              <input id="classifier-model" type="text" bind:value={classifierModel} disabled={pending}>
            {/if}
          </li>
        {/each}
      </ul>
    {/each}
    <h6 class="catalog-label">Your own</h6>
    <ul class="catalog">
      <li><button type="button" class="quiet catalog-row" aria-pressed={chosen === 'endpoint'} onclick={chooseEndpoint}><span class="catalog-name">Another endpoint</span><span class="record">custom classifier</span>{#if chosen === 'endpoint'}<LucideIcon name="check" variant="action" size={14} />{/if}</button>{#if chosen === 'endpoint'}{@render endpointFields()}{/if}</li>
      <li><button type="button" class="quiet catalog-row" aria-pressed={chosen === ''} onclick={chooseNone}><span class="catalog-name">No classifier</span><span class="record">always use the fallback</span>{#if chosen === ''}<LucideIcon name="check" variant="action" size={14} />{/if}</button></li>
    </ul>
    <div class="actions">
      <button type="button" disabled={pending} onclick={saveClassifier}>Save classifier</button>
      {#if settings.classifier.configured && settings.classifier.kind !== 'pooled'}<button type="button" class="quiet" onclick={testClassifier}>Test</button>{/if}
    </div>
    {#if classifierStatus}<p class="support" role="status">{classifierStatus}</p>{/if}

    {#if drafts.length}
      <label for="router-fallback">Fallback</label>
      <select id="router-fallback" bind:value={fallback}>
        <option value="">Lowest known price</option>
        {#each drafts as draft (draft.family + '/' + draft.model)}<option value={draft.draftKey}>{modelLabel(draft.draftKey)}</option>{/each}
      </select>
      <label for="router-confidence">Confidence floor · {Number(confidence).toFixed(2)}</label>
      <input id="router-confidence" type="range" min="0" max="1" step="0.05" bind:value={confidence}>
      <p class="support">Below this confidence, use the fallback.</p>
      <button type="button" class="save" disabled={pending} onclick={saveRoutes}>Save routing rules</button>
    {/if}
    </div>
  </details>
  <div class="configure balancing">
    <div><h5>Account balancing</h5><p class="support" id="balancing-help">Share requests across accounts and skip accounts at their limit.</p></div>
    <Toggle checked={settings.enabled} label="Use account balancing" onchange={toggleRouter} disabled={pending} />
  </div>
</section>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .routing { display: grid; min-width: 0; gap: 10px; align-content: start; overflow-wrap: anywhere; }
  .routing-summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 220px), 1fr)); gap: 12px; margin: 4px 0; padding: 12px; background: var(--faint); border: 1px solid var(--border); border-radius: var(--radius-control); }
  .routing-summary div { min-width: 0; }
  .routing-summary dt { color: var(--muted); font-size: var(--text-13); }
  .routing-summary dd { margin: 4px 0 0; font: var(--text-12) var(--font-mono); }
  .routing-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .routing-head h4 { margin: 0 0 4px; font-size: var(--text-17); font-weight: 600; }
  .selection { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .selection button { display: grid; gap: 5px; padding: 14px; text-align: left; }
  .selection button[aria-pressed="true"] { border-color: var(--muted); background: var(--faint); }
  .selection strong { font-weight: 600; }
  .selection span { color: var(--muted); font-size: var(--text-13); line-height: 1.5; }
  .configure { border-top: 1px solid var(--border); padding-top: 10px; }
  .balancing { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .balancing h5 { margin: 0 0 4px; font-size: var(--text-13); font-weight: 600; }
  summary { cursor: pointer; font-size: var(--text-13); min-height: 24px; }
  .disclosure-body { display: grid; gap: 10px; padding: 12px 0; }
  select { max-width: 100%; }
  @media (max-width: 700px) { .selection { grid-template-columns: 1fr; } }
  .routing-label { color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .empty { padding: 6px 0; }
  .tag, .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .warn { color: var(--ink); }
  .endpoint { display: flex; flex-wrap: wrap; gap: 8px; margin: 0; }
  .group-label { margin: 8px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .catalog-label { margin: 4px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .catalog { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .catalog-row { display: flex; flex-wrap: wrap; width: 100%; align-items: center; gap: 6px 10px; min-height: 34px; padding: 6px 8px; text-align: left; font-size: var(--text-13); }
  .catalog-row[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  .catalog-row:disabled .catalog-name { color: var(--muted); }
  .catalog-name { min-width: 140px; }
  .catalog-row .tag { margin-left: auto; }
  .catalog-row .tag + .tag, .catalog-row .tag + :global(svg) { margin-left: 0; }
  .note { padding: 0 8px 6px; }
  .connection-fields { display: grid; gap: 10px; padding: 10px 8px 14px; }
  .actions { display: flex; gap: 6px; }
  .save { justify-self: start; min-height: 28px; padding: 4px 10px; }
  label { color: var(--muted); font: var(--text-12) var(--font-mono); }
  input[type="password"], input[type="url"], input[type="text"], select { box-sizing: border-box; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: inherit; font-size: var(--text-13); }
  input[type="password"], input[type="url"], select { width: min(420px, 100%); }
  input:focus, select:focus { border-color: var(--muted); outline: 0; }
  input[type="range"] { width: min(300px, 100%); accent-color: var(--ink); }
</style>

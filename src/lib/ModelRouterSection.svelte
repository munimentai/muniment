<script>
  import ChoiceField from './ui/ChoiceField.svelte'
  import Button from './ui/Button.svelte'
  import Toggle from './Toggle.svelte'
  import { pickerGroups } from './provider-catalog.js'
  import { serverDecisionModels } from './classifier-connections.js'
  let { tauri, settings, onsettings, inventory, oninventory, onconnect } = $props()
  let pending = $state(false)
  let error = $state('')
  const automatic = $derived(inventory?.default_provider === 'muniment-router' && inventory?.default_model === 'auto')
  const connections = $derived(settings.classifier_connections ?? [])
  const active = $derived(connections.find(entry => entry.active)?.id ?? '')
  // The connected Ollama server's decision models join the list unconnected.
  // Choosing one saves its connection, tests it and selects it.
  const ollama = $derived(serverDecisionModels(inventory, connections))
  const choices = $derived([
    ...connections.map(entry => ({ value: entry.id, label: entry.name, provider: entry.catalog_id ?? 'jev' })),
    ...ollama.map(entry => ({ value: `server:${entry.provider}:${entry.model}`, label: entry.name, provider: entry.catalogId })),
  ])
  async function connectOllama(entry) {
    const next = await tauri.invoke('model_router_connect_classifier', { catalogId: entry.catalogId, name: entry.name, model: entry.model, baseUrl: entry.baseUrl, apiKey: null, keyProvider: entry.provider })
    onsettings?.(next)
    const saved = next.classifier_connections?.find(row => row.connection?.model === entry.model && row.connection?.base_url === entry.baseUrl)
    if (!saved) throw new Error('The decision model connected, and Muniment could not find it to select.')
    return saved.id
  }
  const models = $derived(pickerGroups(inventory).flatMap(group => group.models).filter(model => model.id !== 'auto'))
  // Assistance picks MCP servers and skills with its own decision model, or
  // with the model routing one when it names none.
  const assist = $derived(settings.assist ?? { enabled: false, connection: '' })
  const routingEntry = $derived(connections.find(entry => entry.active))
  const assistChoices = $derived([
    { value: '', label: routingEntry ? `Same as model routing (${routingEntry.name})` : 'Same as model routing', provider: routingEntry?.catalog_id },
    ...choices,
  ])
  async function setAssist(enabled, id) {
    pending = true; error = ''
    try {
      const entry = ollama.find(row => `server:${row.provider}:${row.model}` === id)
      if (entry) id = await connectOllama(entry)
      if (enabled && !id && !routingEntry) id = connections[0]?.id ?? (ollama[0] ? await connectOllama(ollama[0]) : '')
      onsettings?.(await tauri.invoke('model_router_set_assist', { enabled, id }))
    }
    catch (e) { error = String(e?.message ?? e) }
    finally { pending = false }
  }
  async function select(id) {
    pending = true; error = ''
    try {
      const entry = ollama.find(row => `server:${row.provider}:${row.model}` === id)
      if (entry) id = await connectOllama(entry)
      onsettings?.(await tauri.invoke('model_router_select_classifier', { id }))
      oninventory?.(await tauri.invoke('local_mode_provider_inventory'))
    }
    catch (e) { error = String(e?.message ?? e) }
    finally { pending = false }
  }
  async function toggle(enabled) {
    pending = true; error = ''
    try {
      if (enabled) {
        if (!active && connections[0]) onsettings?.(await tauri.invoke('model_router_select_classifier', { id: connections[0].id }))
        else if (!active && ollama[0]) onsettings?.(await tauri.invoke('model_router_select_classifier', { id: await connectOllama(ollama[0]) }))
        if (!settings.enabled) onsettings?.(await tauri.invoke('model_router_set_enabled', { enabled: true }))
        await tauri.invoke('local_mode_set_default_model', { provider: 'muniment-router', model: 'auto' })
      } else {
        const model = models[0]
        if (!model) throw new Error('Connect an answer model before turning routing off.')
        await tauri.invoke('local_mode_set_default_model', { provider: model.provider, model: model.choice })
      }
      oninventory?.(await tauri.invoke('local_mode_provider_inventory'))
    } catch (e) { error = String(e?.message ?? e) }
    finally { pending = false }
  }
</script>
<section aria-label="Decisions" class="decisions">
  <section aria-label="Model routing" class="job">
    <div class="heading"><div><h5>Model routing</h5><p>Let a decision model choose the model for each message.</p></div><Toggle checked={automatic} label="Use routing" disabled={pending || (!automatic && (!choices.length || !settings.options.length))} onchange={toggle} /></div>
    {#if automatic}
      <ChoiceField label="Decision model" value={active} options={choices} disabled={pending} onchange={select} />
      <p>Account balancing stays on. Your message goes to the selected decision model to choose an answer model.</p>
    {/if}
    {#if !settings.options.length}<p>Connect an account to choose models automatically.</p>{/if}
  </section>
  <section aria-label="Assistance" class="job">
    <div class="heading"><div><h5>Assistance</h5><p>Let a decision model pick the MCP servers and skills for each message.</p></div><Toggle checked={assist.enabled} label="Use assistance" disabled={pending || (!assist.enabled && !choices.length)} onchange={enabled => setAssist(enabled, assist.connection)} /></div>
    {#if assist.enabled}
      <ChoiceField label="Assistance decision model" value={assist.connection} options={assistChoices} disabled={pending} onchange={id => setAssist(true, id)} />
      <p>The composer shows the decision model that assists, and you can turn it off for one message.</p>
    {/if}
  </section>
  {#if pending}<p role="status">Testing the decision model…</p>{/if}
  {#if !choices.length}<p>Connect a decision model in Accounts to use routing or assistance.</p><Button onclick={onconnect}>Connect decision model</Button>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  .decisions, .job { display: grid; gap: 14px; }
  .decisions { gap: 24px; }
  h5 { margin: 0 0 4px; font-size: var(--text-13); font-weight: 600; }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 24px; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); line-height: 1.5; }
</style>

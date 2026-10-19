<script>
  import ChoiceField from './ui/ChoiceField.svelte'
  import Button from './ui/Button.svelte'
  import Toggle from './Toggle.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import { pickerGroups, routerHidden } from './provider-catalog.js'
  import { modelLabel } from './model-label.js'
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
  // A statement tells the decision model when a model should answer, so the
  // statements show once a decision model is connected, one per model the
  // router can pick.
  const statements = $derived(inventory?.router_classifier ? (settings.options ?? []).filter(option => option.model && !routerHidden(inventory).includes(`${option.family}/${option.model}`)) : [])
  let editing = $state('')
  let statement = $state('')
  let saving = $state(false)
  async function saveStatement(route) {
    saving = true; error = ''
    try {
      const routes = (settings.routes ?? []).filter(entry => entry.family !== route.family || entry.model !== route.model)
      routes.push({ key: route.key, family: route.family, model: route.model, description: statement.trim() })
      onsettings?.(await tauri.invoke('model_router_save_routes', { routes, fallback: settings.fallback, minConfidence: settings.min_confidence }))
      editing = ''
    }
    catch (e) { error = String(e?.message ?? e) }
    finally { saving = false }
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
<section aria-label="Roles" class="decisions">
  <Group heading="Roles" info="The decision models that route each message and assist chats. The composer picks the model of one chat.">
    <Row title="Route each message" info="A decision model chooses the model for each message.">
      <Toggle checked={automatic} label="Use routing" disabled={pending || (!automatic && (!choices.length || !settings.options?.length))} onchange={toggle} />
    </Row>
    {#if automatic}
      <Row title="Routing model" info="Account balancing stays on. Your message goes to this model to choose an answer model.">
        <div class="choice"><ChoiceField label="Decision model" hideLabel value={active} options={choices} disabled={pending} onchange={select} /></div>
      </Row>
    {/if}
    <Row title="Assist each message" description="Picks MCP servers and skills, and drives the Browser tab" info="A decision model picks the MCP servers and skills for each message, and drives the Browser tab for a chat, which sends it the page text.">
      <Toggle checked={assist.enabled} label="Use assistance" disabled={pending || (!assist.enabled && !choices.length)} onchange={enabled => setAssist(enabled, assist.connection)} />
    </Row>
    {#if assist.enabled}
      <Row title="Assistance model" info="The composer shows the decision model that assists, and you can turn it off for one message.">
        <div class="choice"><ChoiceField label="Assistance decision model" hideLabel value={assist.connection} options={assistChoices} disabled={pending} onchange={id => setAssist(true, id)} /></div>
      </Row>
    {/if}
    {#if !choices.length}
      <Row title="No decision model" description="Needed for routing and assistance">
        <Button variant="primary" onclick={onconnect}>Connect decision model</Button>
      </Row>
    {/if}
    {#if !settings.options?.length}<p>Connect an account to route messages.</p>{/if}
  </Group>
  {#if statements.length}
    <Group heading="Routing statements" info="Each statement tells the decision model when its model should answer.">
      {#each statements as route (route.key)}
        <Row title={modelLabel(route.model)} description={editing === route.key ? '' : route.description} stacked={editing === route.key}>
          {#if editing === route.key}
            <div class="statement-edit">
              <label for="model-statement">When should this model answer?</label>
              <textarea id="model-statement" rows="3" bind:value={statement}></textarea>
              <div class="actions"><Button disabled={saving} onclick={() => saveStatement(route)}>Save statement</Button><Button onclick={() => { editing = '' }}>Cancel</Button></div>
            </div>
          {:else}<Button aria-label={`Edit statement for ${modelLabel(route.model)}`} onclick={() => { editing = route.key; statement = route.description ?? '' }}>Edit</Button>{/if}
        </Row>
      {/each}
    </Group>
  {/if}
  {#if pending}<p role="status">Testing the decision model…</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  .decisions { display: grid; gap: var(--page-gap); }
  .choice { width: 280px; max-width: 100%; }
  .statement-edit { display: grid; gap: 8px; width: 100%; font-size: var(--text-13); }
  textarea { width: 100%; box-sizing: border-box; resize: vertical; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px 10px; font: inherit; }
  .actions { display: flex; gap: 8px; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); line-height: 1.5; }
</style>

<script>
  import Toggle from './Toggle.svelte'
  import { pickerGroups } from './provider-catalog.js'
  let { tauri, settings, onsettings, inventory, oninventory, onconnect } = $props()
  let pending = $state(false)
  let error = $state('')
  const automatic = $derived(inventory?.default_provider === 'muniment-router' && inventory?.default_model === 'auto')
  const connections = $derived(settings.classifier_connections ?? [])
  const active = $derived(connections.find(entry => entry.active)?.id ?? '')
  const models = $derived(pickerGroups(inventory).flatMap(group => group.models).filter(model => model.id !== 'auto'))
  async function select(id) {
    pending = true; error = ''
    try {
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
<section aria-label="Routing" class="routing">
  <div class="heading"><div><h4>Routing</h4><p>Let a classifier choose the model for each message.</p></div><Toggle checked={automatic} label="Use routing" disabled={pending || (!automatic && (!connections.length || !settings.options.length))} onchange={toggle} /></div>
  {#if automatic}
    <label for="connected-classifier">Classifier model</label>
    <select id="connected-classifier" value={active} disabled={pending || !connections.length} onchange={event => select(event.currentTarget.value)}>
      {#if !active}<option value="" disabled>Choose a connected classifier</option>{/if}
      {#each connections as entry (entry.id)}<option value={entry.id}>{entry.name}</option>{/each}
    </select>
    <p>Account balancing stays on. Your message goes to the selected classifier to choose an answer model.</p>
  {/if}
  {#if !connections.length}<p>Connect a classifier in Accounts to use routing.</p><button type="button" onclick={onconnect}>Connect classifier</button>{/if}
  {#if !settings.options.length}<p>Connect an account to choose models automatically.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  .routing { display: grid; gap: 14px; }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 24px; }
  h4 { margin: 0 0 8px; font-size: var(--text-17); } p { margin: 0; color: var(--muted); font-size: var(--text-13); line-height: 1.5; }
  label { font-size: var(--text-13); } select, button { font: inherit; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 9px 12px; }
  button { justify-self: start; cursor: pointer; } select { width: 100%; }
</style>

<script>
  import Toggle from './Toggle.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import { pickerGroups, modelKey, currentModel } from './provider-catalog.js'
  let { tauri, inventory, settings, onsettings, oninventory } = $props()
  let query = $state('')
  let error = $state('')
  let pending = $state(false)
  let editing = $state('')
  let statement = $state('')
  const groups = $derived(pickerGroups(inventory ? { ...inventory, hidden: [] } : null, query).filter((group) => !group.classifier))
  const current = $derived(currentModel(inventory))
  const hidden = $derived(new Set(inventory?.hidden ?? []))
  function routeFor(model) { return settings?.options?.find((option) => option.model === model.id && (model.choice === option.key || groupFamily(model.provider) === option.family)) }
  function groupFamily(provider) { return provider === 'openai-codex' ? 'openai' : provider === 'claude-bridge' ? 'anthropic' : provider }
  async function run(command, payload, router = false) {
    pending = true; error = ''
    try {
      const next = await tauri.invoke(command, payload)
      if (router) onsettings?.(next)
      oninventory?.(await tauri.invoke('local_mode_provider_inventory'))
      editing = ''
    } catch (failure) { error = String(failure?.message ?? failure) }
    finally { pending = false }
  }
  function saveStatement(route) {
    const routes = (settings.routes ?? []).filter((entry) => entry.family !== route.family || entry.model !== route.model)
    routes.push({ key: route.key, family: route.family, model: route.model, description: statement.trim() })
    return run('model_router_save_routes', { routes, fallback: settings.fallback, minConfidence: settings.min_confidence }, true)
  }
</script>
<section class="catalog" aria-labelledby="models-heading">
  <header><h4 id="models-heading">Models</h4><p>Choose a model. Show it in the composer.</p></header>
  <input type="search" aria-label="Search models" placeholder="Search models or providers" bind:value={query}>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if !groups.length}<p>No models match.</p>{/if}
  {#each groups as group (group.id)}
    <section aria-label={`${group.name} models`}>
      <h5><ProviderLogo provider={group.id.replace('router:', '')} size={16} />{group.name}</h5>
      {#each group.models as model (model.id)}
        {@const route = routeFor(model)}
        {@const provider = route && settings?.enabled ? 'muniment-router' : model.provider}
        {@const choice = route && settings?.enabled ? route.key : model.choice}
        {@const shown = !hidden.has(modelKey(model.provider, model.choice))}
        <div class="model">
          <div class="model-row">
            <div class="identity"><span class="model-id">{model.label}</span><span class="meta">{route ? `${model.accounts || 1} ${model.accounts > 1 ? 'accounts' : 'account'}` : 'Direct'}{#if model.context} · {model.context} context{/if}</span></div>
            {#if current?.provider === provider && current?.model === choice}<span class="meta">Selected</span>{:else}<button disabled={pending || !shown} onclick={() => run('local_mode_set_default_model', { provider, model: choice })}>Use</button>{/if}
            <label class="show"><Toggle checked={shown} disabled={pending} aria-label={`Show ${model.label} in the selector`} onchange={() => run('local_mode_set_model_hidden', { provider: model.provider, model: model.choice, hidden: shown })} />Show</label>
          </div>
          {#if route}
            <details><summary>Routing statement</summary><p class="statement">{route.description}</p>
              {#if editing === route.key}
                <label for="model-statement">When should this model answer?</label><textarea id="model-statement" rows="3" bind:value={statement}></textarea>
                <div class="actions"><button disabled={pending} onclick={() => saveStatement(route)}>Save statement</button><button onclick={() => { editing = '' }}>Cancel</button></div>
              {:else}<button onclick={() => { editing = route.key; statement = route.description }}>Edit statement</button>{/if}
            </details>
          {/if}
        </div>
      {/each}
    </section>
  {/each}
</section>
<style>
  .catalog { display: grid; gap: 14px; border-top: 1px solid var(--border); padding-top: 22px; min-width: 0; }
  h4 { margin: 0 0 4px; font-size: var(--text-17); font-weight: 600; }
  h5 { display: flex; align-items: center; gap: 8px; font-size: var(--text-13); margin: 8px 0; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); line-height: 1.5; }
  .model { padding: 10px 0; border-bottom: 1px solid var(--border); }
  .model-row { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; }
  .identity { flex: 1 1 220px; min-width: 0; display: grid; gap: 4px; }
  .model-id { font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .meta { color: var(--muted); font-size: var(--text-12); }
  .show { display: flex; gap: 6px; align-items: center; font-size: var(--text-12); }
  button { color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); font: inherit; font-size: var(--text-13); padding: 5px 10px; cursor: pointer; }
  button:hover { background: var(--faint); } button:disabled { color: var(--muted); cursor: default; }
  input[type="search"], textarea { width: 100%; box-sizing: border-box; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px 10px; font: inherit; font-size: var(--text-13); }
  textarea { resize: vertical; }
  details { margin-top: 8px; font-size: var(--text-13); } summary { cursor: pointer; min-height: 24px; color: var(--muted); }
  .statement { padding: 6px 0 10px; } .actions { display: flex; gap: 8px; margin-top: 8px; }
</style>

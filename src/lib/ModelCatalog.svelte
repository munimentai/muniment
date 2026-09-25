<script>
  import LucideIcon from './LucideIcon.svelte'
  import Toggle from './Toggle.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import { pickerGroups, modelKey, currentModel } from './provider-catalog.js'
  let { tauri, inventory, settings, onsettings, oninventory } = $props()
  let minContext = $state('0')
  let vision = $state(false)
  let reasoning = $state(false)
  let shownOnly = $state(false)
  let query = $state('')
  let error = $state('')
  let pending = $state(false)
  let visibility = $state({})
  let editing = $state('')
  let statement = $state('')
  const groups = $derived(pickerGroups(inventory ? { ...inventory, hidden: [] } : null, query).filter((group) => !group.classifier).map(group => ({ ...group, models: group.models.filter(model => (!vision || model.images === true) && (!reasoning || model.thinking === true) && (!shownOnly || !hidden.has(modelKey(model.provider, model.choice))) && (!Number(minContext) || contextSize(model.context || routeFor(model)?.context) >= Number(minContext))) })).filter(group => group.models.length))
  const current = $derived(currentModel(inventory))
  const hidden = $derived(new Set(inventory?.hidden ?? []))
  function contextSize(value) { const match = String(value ?? '').replaceAll(',', '').match(/^([\d.]+)\s*([km])?/i); return match ? Number(match[1]) * ({ k: 1000, m: 1000000 }[match[2]?.toLowerCase()] ?? 1) : 0 }
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
  async function setShown(model, shown) {
    const key = modelKey(model.provider, model.choice)
    visibility = { ...visibility, [key]: shown }
    error = ''
    try {
      await tauri.invoke('local_mode_set_model_hidden', { provider: model.provider, model: model.choice, hidden: !shown })
      const next = { ...inventory, hidden: (inventory?.hidden ?? []).filter(entry => entry !== key) }
      if (!shown) next.hidden.push(key)
      inventory = next
      oninventory?.(next)
      // Hiding the saved default must also update the runtime's choice.
      if (!shown && modelKey(next.default_provider, next.default_model) === key) {
        const fallback = currentModel(next)
        if (fallback) {
          await tauri.invoke('local_mode_set_default_model', { provider: fallback.provider, model: fallback.model })
          inventory = { ...inventory, default_provider: fallback.provider, default_model: fallback.model }
          oninventory?.(inventory)
        }
      }
    } catch (failure) { error = String(failure?.message ?? failure) }
    finally {
      const remaining = { ...visibility }
      delete remaining[key]
      visibility = remaining
    }
  }
  function saveStatement(route) {
    const routes = (settings.routes ?? []).filter((entry) => entry.family !== route.family || entry.model !== route.model)
    routes.push({ key: route.key, family: route.family, model: route.model, description: statement.trim() })
    return run('model_router_save_routes', { routes, fallback: settings.fallback, minConfidence: settings.min_confidence }, true)
  }
</script>
<section class="catalog" aria-label="Models">
  <div class="search-row">
    <input type="search" aria-label="Search models" placeholder="Search models or providers" bind:value={query}>
    <details class="filters"><summary><LucideIcon name="sliders-horizontal" size={16} variant="action" />Filters{#if Number(minContext) || vision || reasoning || shownOnly}<span class="count">{Number(!!Number(minContext)) + Number(vision) + Number(reasoning) + Number(shownOnly)}</span>{/if}</summary>
      <div class="filter-fields">
        <label>Minimum context<select bind:value={minContext}><option value="0">Any size</option><option value="32000">32K+</option><option value="128000">128K+</option><option value="200000">200K+</option><option value="1000000">1M+</option></select></label>
        <label><input type="checkbox" bind:checked={vision}>Image input / vision</label>
        <label><input type="checkbox" bind:checked={reasoning}>Reasoning</label>
        <label><input type="checkbox" bind:checked={shownOnly}>Shown in composer</label>
        <p>Capability filters include only models with reported support.</p>
        <button type="button" onclick={() => { minContext = '0'; vision = false; reasoning = false; shownOnly = false }}>Clear filters</button>
      </div>
    </details>
  </div>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if !groups.length}<p>No models match.</p>{/if}
  {#each groups as group (group.id)}
    <section aria-label={`${group.name} models`}>
      <h5><ProviderLogo provider={group.id.replace('router:', '')} size={16} />{group.name}</h5>
      {#each group.models as model (model.id)}
        {@const route = routeFor(model)}
        {@const provider = route && settings?.enabled ? 'muniment-router' : model.provider}
        {@const choice = route && settings?.enabled ? route.key : model.choice}
        {@const key = modelKey(model.provider, model.choice)}
        {@const shown = visibility[key] ?? !hidden.has(key)}
        <div class="model">
          <div class="model-row">
            <div class="identity"><span class="model-id">{model.label}</span><span class="meta">{route ? `${model.accounts || 1} ${model.accounts > 1 ? 'accounts' : 'account'}` : 'Direct'}{#if model.context} · {model.context} context{/if}</span></div>
            {#if current?.provider === provider && current?.model === choice}<span class="meta">Selected</span>{:else}<button disabled={pending || !shown} onclick={() => run('local_mode_set_default_model', { provider, model: choice })}>Use</button>{/if}
            <label class="show"><Toggle checked={shown} disabled={pending || key in visibility} aria-label={`Show ${model.label} in the selector`} onchange={shown => setShown(model, shown)} />Show</label>
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
  .catalog { display: grid; gap: 14px; min-width: 0; }
  .search-row { display: flex; gap: 10px; align-items: start; }
  .filters { position: relative; margin-top: 0; }
  .filters summary { display: flex; align-items: center; gap: 6px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); list-style: none; color: var(--ink); }
  .filters summary::-webkit-details-marker { display: none; }
  .filter-fields { position: absolute; right: 0; z-index: 2; width: min(280px, 65vw); display: grid; gap: 12px; padding: 14px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); box-shadow: 0 8px 24px #0003; }
  .filter-fields label { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .filter-fields select { font: inherit; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px; }
  .count { color: var(--signal); }
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

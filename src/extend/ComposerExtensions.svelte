<script>
  import { tick } from 'svelte'
  import { invocations, invocationQuery } from './catalog.js'
  import McpIcon from './McpIcon.svelte'
  let { tauri, threadId, active = false, draft = $bindable(''), onattach, onmanage } = $props()
  let state = $state({ items: [], threads: {} }), open = $state(false), query = $state(''), selected = $state([]), disabled = $state([]), automatic = $state(false), error = $state(''), loadedThread = null, busy = $state(false), highlighted = $state(0), suggestions = $state([]), dismissed = $state(null), rootElement = $state(), trigger = $state()
  const choices = $derived(invocations(state.items))
  const command = $derived(draft === dismissed ? null : invocationQuery(draft))
  const shown = $derived(choices.filter(item => `${item.name} ${item.description}`.toLowerCase().includes(command ?? query.toLowerCase())).slice(0, 20))
  const servers = $derived(state.items.filter(i => i.enabled !== false).flatMap(i => i.kind === 'mcp' ? [{ id: i.id, name: i.name }] : Object.keys(i.servers || {}).map(name => ({ id: `${i.id}:${name}`, name: `${i.name} · ${name}` }))))
  async function call(action, data = {}) { return tauri.invoke('extend_command', { action, data }) }
  async function refresh() {
    try {
      const next = await call('read'); if (!next?.items) return
      state = next
      if (loadedThread !== threadId) {
        const rules = state.threads[threadId] || {}; selected = rules.selected || []; disabled = rules.disabled || []; automatic = !!rules.automatic; loadedThread = threadId
      }
    } catch { if (open) error = 'Extensions could not be loaded.' }
  }
  $effect(() => { const id = threadId; void id; void refresh() })
  $effect(() => { if (command !== null) void refresh() })
  async function save() {
    if (!threadId) return
    try { await call('thread', { threadId, selected, disabled, automatic }); error = '' } catch (e) { error = String(e) }
  }
  async function choose(item) { if (!selected.includes(item.id)) selected = [...selected, item.id]; if (command !== null) draft = ''; open = false; await save() }
  async function toggle(id, checked) { disabled = checked ? disabled.filter(value => value !== id) : [...disabled, id]; await save() }
  export function prepare(prompt) {
    if (!state.items.length) return
    return prepareSelected(prompt)
  }
  async function prepareSelected(prompt) {
    await refresh()
    const id = threadId || await tauri.invoke('chat_current_thread') || await tauri.invoke('chat_new_thread')
    await call('thread', { threadId: id, selected, disabled, automatic })
    if (automatic) {
      busy = true
      try { const result = await call('route', { threadId: id, prompt }); suggestions = result.selected; } finally { busy = false }
    }
  }
  export function handleKey(event) {
    if (command === null || !shown.length) return false
    if (event.key === 'Escape') { dismissed = draft; event.preventDefault(); return true }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { highlighted = (highlighted + (event.key === 'ArrowDown' ? 1 : shown.length - 1)) % shown.length; event.preventDefault(); return true }
    if (event.key === 'Enter' || event.key === 'Tab') { event.preventDefault(); void choose(shown[highlighted % shown.length]); return true }
    return false
  }
  function keys(event) {
    if (event.key === 'Escape') { open = false; query = ''; trigger?.focus(); event.stopPropagation() }
  }
</script>
<svelte:window onkeydown={event => { if (open) keys(event) }} onpointerdown={event => { if (open && !rootElement?.contains(event.target)) open = false }} />
<div class="extensions" bind:this={rootElement} role="group" aria-label="Chat extensions">
  {#if !active}<button type="button" class="trigger" bind:this={trigger} aria-label="Tools and attachments" aria-expanded={open} onclick={async () => { open = !open; if (open) { await refresh(); await tick() } }}><span aria-hidden="true">⋮</span></button>{/if}
  {#if selected.length}<div class="chips" aria-label="Selected extensions">{#each selected as id}{@const item = choices.find(i => i.id === id)}{#if item}<button type="button" disabled={active} aria-label={`Remove ${item.name}`} onclick={() => { selected = selected.filter(v => v !== id); void save() }}>{item.name} ×</button>{/if}{/each}</div>{/if}
  {#if open || (command !== null && shown.length && !active)}
    <section class="menu" aria-label="Tools and attachments menu">
      {#if open}<button type="button" onclick={() => { open = false; onattach() }}>Add files</button><input type="search" aria-label="Search available extensions" placeholder="Search skills and plugins" bind:value={query} />{/if}
      {#if open && servers.length}<h4>MCP servers</h4>{#each servers as server}<label class="server"><McpIcon /><span>{server.name}</span><input type="checkbox" checked={!disabled.includes(server.id)} onchange={e => toggle(server.id, e.currentTarget.checked)} /></label>{/each}{/if}
      {#if shown.length}<h4>Skills and plugins</h4>{#each shown as item, index}<button type="button" class="choice" class:highlighted={command !== null && index === highlighted % shown.length} onclick={() => choose(item)}><strong>{item.name}</strong><span>{item.kind} · {item.description}</span></button>{/each}{:else if open}<p>No matching skills or plugins. Install them in Extend.</p>{/if}
      {#if open}<label class="server"><span>Choose relevant extensions automatically</span><input type="checkbox" bind:checked={automatic} onchange={save} /></label><p>Uses your configured classifier. Disabled servers stay off.</p><button type="button" onclick={() => { open = false; onmanage() }}>Manage extensions</button><button type="button" onclick={() => open = false}>Close</button>{/if}
    </section>
  {/if}
  {#if suggestions.length}<span class="suggestions" aria-label="Automatically selected extensions">Auto: {suggestions.map(id => choices.find(i => i.id === id)?.name || servers.find(s => s.id === id)?.name).filter(Boolean).join(', ')}</span>{/if}
  {#if busy}<span role="status">Selecting extensions…</span>{/if}
  {#if error}<span role="alert">{error}</span>{/if}
</div>
<style>
  .extensions { position: relative; display: flex; align-items: center; gap: 5px; } .trigger { background: transparent; border: 0; padding: 0; font-size: var(--text-22); width: 28px; height: 28px; } .menu { position: absolute; bottom: 38px; left: 0; width: min(360px, 75vw); max-height: 440px; overflow: auto; display: grid; gap: 8px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); box-shadow: var(--shadow-overlay); z-index: 20; } .menu input[type=search] { min-width: 0; padding: 8px; background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); } h4, p { margin: 0; font-size: var(--text-12); } p, .choice span { color: var(--muted); } .server { display: flex; align-items: center; gap: 8px; font-size: var(--text-12); } .server span { flex: 1; } .choice { display: grid; gap: 3px; text-align: left; } .choice span { font-size: var(--text-provenance); } .highlighted { background: var(--border); } .suggestions { font-size: var(--text-provenance); color: var(--muted); } .chips { display: flex; flex-wrap: wrap; gap: 4px; max-width: 240px; } .chips button { font-size: var(--text-provenance); }
</style>

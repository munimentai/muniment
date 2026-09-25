<script>
  import Toggle from '../lib/Toggle.svelte'
  import { onMount, tick } from 'svelte'
  import { commandChoices, selectedCommands, invocationQuery } from './commands.js'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from '../lib/composer-panels.js'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import McpIcon from './McpIcon.svelte'
  import ProviderIcon from './ProviderIcon.svelte'
  let catalog = $state([])
  let { tauri, threadId, active = false, draft = $bindable(''), commandNames = $bindable([]), selection = $bindable({ enabled: [], blocked: [], automatic: false }), onmanage } = $props()
  let state = $state({ items: [] }), open = $state(false), branch = $state(''), error = $state(''), busy = $state(false), highlighted = $state(0), dismissed = $state(null), rootElement = $state(), trigger = $state()
  const choices = $derived(commandChoices(state.items))
  const command = $derived(draft === dismissed ? null : invocationQuery(draft))
  const shown = $derived(choices.filter(item => `${item.command} ${item.description || ''}`.toLowerCase().includes(command || '')).slice(0, 20))
  const servers = $derived(state.items.filter(i => i.enabled !== false).flatMap(i => i.kind === 'mcp' ? [{ id: i.id, name: i.name, entry: catalog.find(entry => entry.source === i.source) }] : Object.keys(i.servers || {}).map(name => ({ id: `${i.id}:${name}`, name: `${i.name} · ${name}` }))))
  async function call(action, data = {}) { return tauri.invoke('extend_command', { action, data }) }
  async function refresh() {
    try { const next = await call('read'); if (next?.items) state = next }
    catch { if (open) error = 'Extensions could not be loaded.' }
  }
  onMount(() => {
    const follow = event => { if (event.detail && event.detail !== 'extend' && event.detail !== 'commands') dismissed = draft; open = event.detail === 'extend'; if (open) { branch = ''; void refresh(); void import('./catalog.js').then(module => catalog = module.catalog) } }
    window.addEventListener(COMPOSER_PANEL_EVENT, follow)
    void refresh()
    return () => window.removeEventListener(COMPOSER_PANEL_EVENT, follow)
  })
  $effect(() => { void threadId; branch = ''; open = false })
  $effect(() => { commandNames = choices.map(item => item.command) })
  $effect(() => { if (command !== null) { highlighted = 0; openComposerPanel('commands'); void refresh() } })
  function close() { openComposerPanel(null); open = false }
  function choose(item) {
    const token = `/${item.command} `
    draft = command !== null ? token : `${draft}${draft && !/\s$/.test(draft) ? ' ' : ''}${token}`
    dismissed = draft
    close()
    document.getElementById('composer-message')?.focus()
  }
  function toggle(id, checked) {
    selection.enabled = checked ? [...new Set([...selection.enabled, id])] : selection.enabled.filter(value => value !== id)
    selection.blocked = checked ? selection.blocked.filter(value => value !== id) : [...new Set([...selection.blocked, id])]
  }
  export function prepare(prompt) {
    if (!state.items.length) return
    return prepareSelected(prompt)
  }
  async function prepareSelected(prompt) {
    await refresh()
    const id = threadId || await tauri.invoke('chat_current_thread') || await tauri.invoke('chat_new_thread')
    await call('turn', { threadId: id, selected: [...selectedCommands(prompt, choices), ...selection.enabled], disabled: selection.blocked, automatic: selection.automatic })
    if (selection.automatic) {
      busy = true
      try { await call('route', { threadId: id, prompt }) } finally { busy = false }
    }
  }
  export function submitted() { selection.enabled = []; selection.blocked = []; selection.automatic = false; close() }
  export function handleKey(event) {
    if (command === null || !shown.length) return false
    if (event.key === 'Escape') { dismissed = draft; event.preventDefault(); return true }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { highlighted = (highlighted + (event.key === 'ArrowDown' ? 1 : shown.length - 1)) % shown.length; event.preventDefault(); return true }
    if (event.key === 'Enter' || event.key === 'Tab') { event.preventDefault(); choose(shown[highlighted % shown.length]); return true }
    return false
  }
  async function expand(kind) { branch = branch === kind ? '' : kind; await tick(); rootElement?.querySelector('.submenu button, .submenu input')?.focus() }
  function keys(event) {
    if (event.key === 'Escape') { close(); dismissed = draft; trigger?.focus(); event.stopPropagation() }
    if (event.key === 'ArrowLeft' && branch) { const previous = branch; branch = ''; rootElement?.querySelector(`[data-branch="${previous}"]`)?.focus(); event.preventDefault() }
  }
</script>
<svelte:window onkeydown={event => { if (open) keys(event) }} onpointerdown={event => { if (open && !rootElement?.contains(event.target)) close() }} />
<div class="extensions" bind:this={rootElement} role="group" aria-label="Turn extensions">
  {#if !active}<button type="button" class="trigger" bind:this={trigger} aria-label="Extensions" aria-haspopup="dialog" aria-expanded={open} onclick={() => openComposerPanel(open ? null : 'extend')}><LucideIcon name="ellipsis" variant="action" size={14} /></button>{/if}
  {#if open}
    <div data-composer-panel data-panel="extend" data-panel-variant="overlay" class="menu" role="dialog" tabindex="-1" aria-label="Extensions for this turn">
      <div class="tree" class:expanded={!!branch}>
        <div class="branches">
          {#each [['mcp', 'MCPs'], ['plugin', 'Plugins'], ['skill', 'Skills']] as [kind, label]}
            <button type="button" data-branch={kind} aria-expanded={branch === kind} onclick={() => expand(kind)} onkeydown={event => { if (event.key === 'ArrowRight') { event.preventDefault(); void expand(kind) } }}>{#if kind === 'mcp'}<McpIcon />{:else}<LucideIcon name={kind === 'skill' ? 'pencil-sparkles' : 'unplug'} variant="action" size={16} />{/if}<span>{label}</span><LucideIcon name="chevron-right" size={14} variant="action" /></button>
          {/each}
        </div>
        {#if branch}<div class="submenu" aria-label={branch === 'mcp' ? 'Available MCPs' : branch === 'plugin' ? 'Available plugins' : 'Available skills'}>
          {#if branch === 'mcp'}
            {#each servers as server}<div class="server">{#if server.entry}<ProviderIcon entry={server.entry} size={22} />{:else}<McpIcon />{/if}<span>{server.name}</span><Toggle checked={selection.enabled.includes(server.id)} label={`Use ${server.name} for this turn`} onchange={checked => toggle(server.id, checked)} /></div>{/each}
            {#if !servers.length}<p>No MCPs installed.</p>{/if}
            <div class="automatic"><span>Auto-select for this turn</span><Toggle checked={selection.automatic} label="Auto-select for this turn" onchange={checked => selection.automatic = checked} /></div>
          {:else}
            {#each choices.filter(item => item.kind === branch) as item}<button type="button" class="choice" onclick={() => choose(item)}><span>/{item.command}</span><small>{item.description || item.name}</small></button>{/each}
            {#if !choices.some(item => item.kind === branch)}<p>No {branch === 'plugin' ? 'plugins' : 'skills'} installed.</p>{/if}
          {/if}
        </div>{/if}
      </div>
      <button type="button" class="manage" onclick={() => { close(); onmanage() }}>Manage extensions</button>
    </div>
  {:else if command !== null && shown.length && !active}
    <section data-composer-panel data-panel="extend" data-panel-variant="overlay" class="commands" aria-label="Slash commands">
      {#each shown as item, index}<button type="button" class="choice" class:highlighted={index === highlighted % shown.length} onclick={() => choose(item)}><span>/{item.command}</span><small>{item.kind} · {item.description || item.name}</small></button>{/each}
    </section>
  {/if}
  {#if busy}<span role="status">Selecting extensions…</span>{/if}
  {#if error}<span role="alert">{error}</span>{/if}
</div>
<style>
  .extensions { display: flex; align-items: center; gap: 5px; }
  button { font: inherit; font-size: var(--text-13); color: var(--ink); cursor: pointer; border: 0; border-radius: var(--radius-control); background: transparent; padding: 7px 9px; }
  button:hover, button:focus-visible, .highlighted { background: var(--faint); }
  .trigger { flex: none; display: inline-flex; align-items: center; justify-content: center; min-width: 24px; min-height: 24px; padding: 3px 4px; border: 1px solid transparent; font: var(--text-12) var(--font-mono); background: transparent; }
  .trigger:hover { background: var(--faint); }
  .menu { width: min(520px, 100%); padding: 6px; }
  .menu:has(.tree:not(.expanded)) { width: min(240px, 100%); }
  .tree { display: grid; grid-template-columns: 1fr; }
  .tree.expanded { grid-template-columns: minmax(100px, 1fr) minmax(0, 2fr); }
  .branches button { display: flex; align-items: center; width: 100%; gap: 8px; text-align: left; }
  .branches button span:first-of-type { flex: 1; }
  .branches button[aria-expanded=true] { background: var(--faint); }
  .submenu { min-width: 0; border-left: 1px solid var(--border); padding-left: 6px; overflow-y: auto; max-height: 42vh; }
  .server, .automatic { display: flex; align-items: center; gap: 8px; padding: 7px; font-size: var(--text-13); }
  .server span, .automatic span { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .automatic { border-top: 1px solid var(--border); margin-top: 5px; color: var(--muted); }
  .manage { display: block; width: 100%; text-align: left; border-top: 1px solid var(--border); margin-top: 5px; padding-top: 10px; }
  .choice { display: grid; width: 100%; gap: 3px; text-align: left; }
  .choice span { color: var(--reference); }
  small, p { color: var(--muted); font-size: var(--text-12); }
  p { padding: 8px; }
  .commands { width: min(380px, 100%); padding: 6px; overflow-y: auto; }
</style>

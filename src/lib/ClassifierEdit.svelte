<script>
  // Edits a saved classifier connection: its name, model and URL. The key field
  // starts blank, because Settings never reads a key back, and a blank key keeps
  // the saved one.
  let { connection, tauri, onsaved, oncancel } = $props()
  // svelte-ignore state_referenced_locally
  let name = $state(connection.name)
  // svelte-ignore state_referenced_locally
  let model = $state(connection.connection.model)
  // svelte-ignore state_referenced_locally
  let url = $state(connection.connection.base_url ?? '')
  let key = $state('')
  let pending = $state(false)
  let error = $state('')
  // TypeSafe's hosted API needs no URL, so its field may stay blank.
  const hosted = $derived(connection.connection.kind === 'typesafe')
  const field = (part) => `classifier-${connection.id}-${part}`
  async function save() {
    pending = true; error = ''
    try {
      const next = await tauri.invoke('model_router_update_classifier', { id: connection.id, name, model, baseUrl: url, apiKey: key || null })
      key = ''
      onsaved(next)
    } catch (failure) { error = String(failure?.message ?? failure) }
    finally { pending = false }
  }
</script>
<form class="classifier-edit" aria-label={`Edit ${connection.name}`} onsubmit={(event) => { event.preventDefault(); save() }}>
  <div class="fields">
    <div class="field"><label for={field('name')}>Connection name</label><input id={field('name')} bind:value={name} disabled={pending}></div>
    <div class="field"><label for={field('model')}>Model ID</label><input id={field('model')} bind:value={model} disabled={pending}></div>
    <div class="field wide"><label for={field('url')}>Decision model URL{hosted ? ' (optional)' : ''}</label><input id={field('url')} type="url" bind:value={url} disabled={pending}></div>
    <div class="field wide"><label for={field('key')}>API key</label><input id={field('key')} type="password" autocomplete="off" placeholder="Leave blank to keep the saved key" bind:value={key} disabled={pending}></div>
  </div>
  <p class="support">A new model, URL or key runs the connection test before it saves.</p>
  <div class="actions">
    <button type="submit" disabled={pending || !name.trim() || !model.trim() || (!hosted && !url.trim())}>{pending ? 'Testing connection…' : 'Save'}</button>
    <button type="button" disabled={pending} onclick={oncancel}>Cancel</button>
  </div>
  {#if error}<p role="alert">{error}</p>{/if}
</form>
<style>
  .classifier-edit { display: grid; gap: 12px; max-width: 680px; min-width: 0; padding: 4px 4px 14px 30px; font-size: var(--text-13); }
  .fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px 16px; }
  .field { display: grid; gap: 5px; min-width: 0; }
  .wide { grid-column: 1 / -1; }
  @media (max-width: 600px) { .fields { grid-template-columns: 1fr; } .classifier-edit { padding-left: 4px; } }
  .actions { display: flex; gap: 8px; }
  button, input { font: inherit; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 7px 10px; }
  button { cursor: pointer; } button:disabled { opacity: .5; cursor: default; }
  .support, label { color: var(--muted); font-size: var(--text-13); }
  p { margin: 0; line-height: 1.5; } input { width: 100%; box-sizing: border-box; }
</style>

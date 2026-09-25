<script>
  import { CONNECTION_METHODS, LOCAL_SETUP } from './classifier-connections.js'
  let { entry, tauri, onconnected } = $props()
  let method = $state('')
  let name = $state('')
  let model = $state('')
  let url = $state('')
  let key = $state('')
  let accountId = $state('')
  let pending = $state(false)
  let error = $state('')
  let seen = ''
  function choose(next) {
    method = next
    name = `${entry.name} · ${CONNECTION_METHODS[next].name}`
    model = CONNECTION_METHODS[next].model || entry.model
    url = CONNECTION_METHODS[next].url || ''
    if (next === 'server' && entry.id === 'kev') url = 'http://127.0.0.1:8009/v1/systemone'
    key = ''; error = ''
  }
  $effect(() => { if (seen !== entry.id) { seen = entry.id; choose(entry.methods[0]) } })
  async function open(url) { try { await tauri.invoke('local_mode_open_url', { url }) } catch { error = 'The browser could not open. Use the link shown below.' } }
  async function connect() {
    pending = true; error = ''
    try {
      const baseUrl = method === 'cloudflare' ? `https://api.cloudflare.com/client/v4/accounts/${accountId.trim()}/ai/run` : url
      const settings = await tauri.invoke('model_router_connect_classifier', { catalogId: entry.id, name, model, baseUrl, apiKey: key || null })
      key = ''; onconnected(settings)
    } catch (e) { error = String(e?.message ?? e) }
    finally { pending = false }
  }
</script>
<p class="support">{entry.note}</p>
<div class="methods" role="group" aria-label="Connection method">
  {#each entry.methods as id}<button disabled={pending} type="button" aria-pressed={method === id} onclick={() => choose(id)}>{CONNECTION_METHODS[id].name}</button>{/each}
</div>
{#if method === 'download'}
  <p>Download the project and start its server, then return here to connect. Muniment does not install Python or model weights.</p>
  {#if !entry.compatible}<p>This project needs an adapter that accepts System One requests. Downloading its weights alone does not connect it.</p>{/if}
  {#if LOCAL_SETUP[entry.id]}<pre>{LOCAL_SETUP[entry.id]}</pre>{/if}
  <div class="methods"><button type="button" onclick={() => open(`${entry.repo}/archive/HEAD.zip`)}>Download source package</button><button type="button" onclick={() => open(entry.repo)}>Open setup guide</button></div>
  <p class="support">The source package does not include model weights. Follow the setup guide to download the weights and start the server.</p>
  <p class="support">{entry.repo}</p>
  <button type="button" onclick={() => choose('server')}>Connect the running server</button>
{:else}
  {#if method === 'server'}<p class="support">Enter the full System One URL for a local or hosted server. Chat completion endpoints do not work here.</p>{/if}
  <p class="support">When routing is on, Muniment sends the message to this classifier. The connection test sends a sample request.</p>
  <label for="classifier-name">Connection name</label><input id="classifier-name" bind:value={name} disabled={pending}>
  {#if method === 'cloudflare'}
    <label for="cloudflare-account">Cloudflare account ID</label><input id="cloudflare-account" bind:value={accountId} disabled={pending}>
  {:else}
    <label for="classifier-url">Classifier URL</label><input id="classifier-url" type="url" bind:value={url} disabled={pending}>
  {/if}
  <label for="classifier-model">Model ID</label><input id="classifier-model" bind:value={model} disabled={pending}>
  <label for="classifier-key">API key{CONNECTION_METHODS[method]?.key ? '' : ' (optional)'}</label><input id="classifier-key" type="password" autocomplete="off" bind:value={key} disabled={pending}>
  <button type="button" disabled={pending || !name.trim() || !model.trim() || (method === 'cloudflare' ? !/^[a-f0-9]{32}$/i.test(accountId.trim()) : !url.trim()) || (CONNECTION_METHODS[method]?.key && !key.trim())} onclick={connect}>{pending ? 'Testing connection…' : 'Test and connect'}</button>
{/if}
{#if error}<p role="alert">{error}</p>{/if}
<style>
  .methods { display: flex; flex-wrap: wrap; gap: 8px; }
  button, input { font: inherit; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 9px 12px; }
  button { cursor: pointer; } button[aria-pressed="true"] { border-color: var(--signal); background: var(--signal-soft); }
  button:disabled { opacity: .5; cursor: default; } .support, label { color: var(--muted); font-size: var(--text-13); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; padding: 16px; background: var(--faint); font: var(--text-12) var(--font-mono); }
  p { margin: 0; line-height: 1.5; } input { width: 100%; box-sizing: border-box; }
</style>

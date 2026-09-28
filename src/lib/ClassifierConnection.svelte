<script>
  import { CONNECTION_METHODS, LOCAL_SETUP, CLASSIFIER_GUIDES } from './classifier-connections.js'
  let { entry, tauri, onconnected } = $props()
  const guide = $derived(CLASSIFIER_GUIDES[entry.id])
  let threshold = $state(0.6)
  let scoreKind = $state('native')
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
  $effect(() => { if (seen !== entry.id) { seen = entry.id; choose(entry.methods[0]); scoreKind = entry.id === 'nimble' ? 'probability' : 'native' } })
  async function open(url) { try { await tauri.invoke('local_mode_open_url', { url }) } catch { error = 'The browser could not open. Use the link shown below.' } }
  async function connect() {
    pending = true; error = ''
    try {
      const baseUrl = method === 'cloudflare' ? `https://api.cloudflare.com/client/v4/accounts/${accountId.trim()}/ai/run` : url
      const settings = await tauri.invoke('model_router_connect_classifier', { catalogId: entry.id, name, model, baseUrl, apiKey: key || null, minConfidence: Number(threshold), scoreKind })
      key = ''; onconnected(settings)
    } catch (e) { error = String(e?.message ?? e) }
    finally { pending = false }
  }
</script>
<div class="classifier-connection">
<p class="support">{entry.note}</p>
{#if guide}
  <section class="setup-guide" aria-label={`${entry.name} connection instructions`}>
    <h5>Connect {entry.name}</h5>
    <ol>{#each guide.steps as step}<li>{step}</li>{/each}</ol>
    {#if LOCAL_SETUP[entry.id]}<pre>{LOCAL_SETUP[entry.id]}</pre>{/if}
    <p class="support">{guide.detail}</p>
    <div class="methods">{#each guide.links as link}<a href={link.url} onclick={(event) => { event.preventDefault(); open(link.url) }}>{link.label}</a>{/each}</div>
  </section>
{/if}
<div class="methods" role="group" aria-label="Connection method">
  {#each entry.methods as id}<button disabled={pending} type="button" aria-pressed={method === id} onclick={() => choose(id)}>{CONNECTION_METHODS[id].name}</button>{/each}
</div>
{#if method === 'download'}
  <p>Download the project and start its server, then return here to connect. Muniment does not install Python or model weights.</p>
  {#if !entry.compatible}<p>This project needs an adapter that accepts System One requests. Downloading its weights alone does not connect it.</p>{/if}
  {#if LOCAL_SETUP[entry.id] && !guide}<pre>{LOCAL_SETUP[entry.id]}</pre>{/if}
  <div class="methods"><button type="button" onclick={() => open(`${entry.repo}/archive/HEAD.zip`)}>Download source package</button><button type="button" onclick={() => open(entry.repo)}>Open setup guide</button></div>
  <p class="support">The source package does not include model weights. Follow the setup guide to download the weights and start the server.</p>
  <p class="support">{entry.repo}</p>
  <button type="button" onclick={() => choose('server')}>Connect the running server</button>
{:else}
  {#if method === 'server'}<p class="support">Enter the full System One URL for a local or hosted server. Chat completion endpoints do not work here.</p>{/if}
  <p class="support">When routing is on, Muniment sends bounded task context to this classifier. The connection test sends a sample request.</p>
  <div class="fields">
    <div class="field"><label for="classifier-name">Connection name</label><input id="classifier-name" bind:value={name} disabled={pending}></div>
    <div class="field"><label for="classifier-model">Model ID</label><input id="classifier-model" bind:value={model} disabled={pending}></div>
    <div class="field wide">
      {#if method === 'cloudflare'}
        <label for="cloudflare-account">Cloudflare account ID</label><input id="cloudflare-account" bind:value={accountId} disabled={pending}>
      {:else}
        <label for="classifier-url">Classifier URL</label><input id="classifier-url" type="url" bind:value={url} disabled={pending}>
      {/if}
    </div>
    <div class="field wide"><label for="classifier-key">API key{CONNECTION_METHODS[method]?.key ? '' : ' (optional)'}</label><input id="classifier-key" type="password" autocomplete="off" bind:value={key} disabled={pending}></div>
  </div>
  <details><summary>Decision threshold</summary>
    <p class="support">This threshold belongs to this connection. Scores from different classifiers are not equivalent. Validate it against your own tasks.</p>
    <div class="field"><label for="classifier-score">Score to check</label><select id="classifier-score" bind:value={scoreKind}><option value="native">Native confidence</option><option value="probability">Chosen option probability</option></select></div>
    <div class="field"><label for="classifier-threshold">Minimum score</label><input id="classifier-threshold" type="number" min="0" max="1" step="0.05" bind:value={threshold}></div>
  </details>
  <button type="button" disabled={pending || !name.trim() || !model.trim() || (method === 'cloudflare' ? !/^[a-f0-9]{32}$/i.test(accountId.trim()) : !url.trim()) || (CONNECTION_METHODS[method]?.key && !key.trim())} onclick={connect}>{pending ? 'Testing connection…' : 'Test and connect'}</button>
{/if}
{#if error}<p role="alert">{error}</p>{/if}
</div>
<style>
  .classifier-connection { display: grid; gap: 12px; max-width: 680px; min-width: 0; font-size: var(--text-13); }
  .fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px 16px; }
  .field { display: grid; gap: 5px; min-width: 0; }
  .wide { grid-column: 1 / -1; }
  @media (max-width: 600px) { .fields { grid-template-columns: 1fr; } }
  .methods { display: flex; flex-wrap: wrap; gap: 8px; }
  .setup-guide { display: grid; gap: 12px; } h5 { margin: 0; font-size: var(--text-15); } ol { margin: 0; padding-left: 20px; line-height: 1.6; } a { color: var(--ink); text-decoration: underline; } details { display: grid; gap: 10px; }
  button, input, select { font: inherit; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 7px 10px; }
  button { cursor: pointer; justify-self: start; } button[aria-pressed="true"] { border-color: var(--muted); background: var(--faint); }
  button:disabled { opacity: .5; cursor: default; } .support, label { color: var(--muted); font-size: var(--text-13); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; margin: 0; padding: 12px; background: var(--faint); font: var(--text-12) var(--font-mono); }
  p { margin: 0; line-height: 1.5; } input { width: 100%; box-sizing: border-box; }
</style>

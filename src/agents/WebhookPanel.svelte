<script>
  // A webhook routine's address: its URLs on the machine's remote-access
  // listener, the listener certificate's fingerprint for senders to pin,
  // whether the listener runs, and the secret, shown once when core issues
  // or rotates it. Core stores only what verifies the secret.
  import { onMount } from 'svelte'
  import Button from '../lib/ui/Button.svelte'
  import { agentRequest } from './agent-runtime.js'

  let { tauri, runtime, agentId, routineId = null, created = null, disabled = false } = $props()
  let hook = $state(created?.hook ?? null)
  let secret = $state(created?.secret ?? '')
  let loading = $state(false)
  let busy = $state(false)
  let error = $state('')
  let copied = $state('')

  onMount(() => { if (!hook && routineId) void load() })

  async function load() {
    loading = true
    error = ''
    try { hook = (await agentRequest(tauri, runtime, 'routine.webhook_info', { agent_id: agentId, routine_id: routineId }))?.hook ?? null } catch (failure) { error = String(failure?.message ?? failure) } finally { loading = false }
  }

  async function rotate() {
    busy = true
    error = ''
    try {
      const answer = await agentRequest(tauri, runtime, 'routine.webhook_rotate', { agent_id: agentId, routine_id: routineId })
      secret = answer?.secret ?? ''
      hook = answer?.hook ?? hook
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }

  async function copy(label, text) {
    try { await navigator.clipboard.writeText(text); copied = label } catch { copied = '' }
  }
</script>

<section class="webhook" aria-label="Webhook address">
  {#if !routineId && !created}
    <p class="note">Save to get the address and its secret.</p>
  {:else if loading}
    <p class="note" role="status">Loading the address…</p>
  {:else if hook}
    {#if !hook.listening}<p class="warning" role="note">This machine does not accept connections, so nothing reaches this address. Turn on Accept connections in Settings, Machines.</p>{/if}
    <div class="field">
      <span class="label">Address</span>
      {#each hook.urls ?? [] as url (url)}
        <div class="value"><code>{url}</code><Button variant="outline" onclick={() => copy(url, url)}>{copied === url ? 'Copied' : 'Copy'}</Button></div>
      {:else}<p class="note">No address yet. Add an address in Settings, Machines.</p>{/each}
    </div>
    <div class="field"><span class="label">Certificate SHA-256</span><code class="wrap">{hook.certificate_sha256}</code><small>The certificate is self-signed. Senders pin this fingerprint or skip verification.</small></div>
    <p class="record">Hook {hook.hook_id} · {hook.preset === 'github' ? 'GitHub' : hook.preset === 'forgejo' ? 'Forgejo' : 'Any sender'}</p>
  {/if}
  {#if secret}
    <div class="secret" role="note">
      <span class="label">Secret</span>
      <div class="value"><code class="wrap">{secret}</code><Button variant="outline" onclick={() => copy('secret', secret)}>{copied === 'secret' ? 'Copied' : 'Copy'}</Button></div>
      <small>Copy it now. It does not show again. Send it as a bearer token or sign each delivery with it.</small>
    </div>
  {/if}
  {#if routineId || created}<div class="actions"><Button variant="outline" disabled={disabled || busy || loading} onclick={rotate}>{busy ? 'Rotating…' : secret ? 'Rotate again' : 'Rotate secret'}</Button></div>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .webhook { display: grid; gap: 10px; }
  .field, .secret { display: grid; gap: 4px; }
  .label { color: var(--muted); font-size: var(--text-12); }
  .value { display: flex; align-items: center; gap: 8px; min-width: 0; }
  code { flex: 1; min-width: 0; padding: 6px; border-radius: var(--radius-control); background: var(--faint); font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .wrap { word-break: break-all; }
  .secret { padding: 10px; border: 1px solid var(--warning); border-radius: var(--radius-control); }
  small, .note, .record { margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.5; }
  .record { font-family: var(--font-mono); }
  .warning { margin: 0; padding: 8px 10px; border: 1px solid var(--warning); border-radius: var(--radius-control); font-size: var(--text-12); line-height: 1.5; }
  .actions { display: flex; gap: 6px; }
  .error { margin: 0; color: var(--danger); font-size: var(--text-12); }
</style>

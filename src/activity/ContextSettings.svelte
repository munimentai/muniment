<script>
  import Toggle from '../lib/Toggle.svelte'
  import { onMount } from 'svelte'
  let { tauri } = $props()
  let enabled = $state(true)
  let reserveTokens = $state(16384)
  let keepRecentTokens = $state(20000)
  let ready = $state(false)
  let saving = $state(false)
  let error = $state('')
  let status = $state('')
  onMount(async () => {
    try {
      const value = await tauri.invoke('context_settings')
      enabled = value.enabled; reserveTokens = value.reserveTokens; keepRecentTokens = value.keepRecentTokens
      ready = true
    } catch { error = 'Context settings could not be read.' }
  })
  async function save() {
    saving = true; error = ''; status = ''
    try {
      await tauri.invoke('context_settings_save', { enabled, reserveTokens, keepRecentTokens })
      status = 'Saved. Applies to the next message.'
    } catch (failure) { error = String(failure?.message ?? failure) }
    finally { saving = false }
  }
</script>
<section aria-labelledby="context-heading">
  <h3 id="context-heading">Conversation context</h3>
  <p>Compaction summarizes older messages so long conversations can continue. The full conversation stays in your history.</p>
  {#if ready}
    <form onsubmit={(event) => { event.preventDefault(); void save() }}>
      <label class="toggle"><Toggle bind:checked={enabled} disabled={saving} /> Compact automatically</label>
      <p>Compaction starts before the model runs out of room. If disabled, a full context can stop a reply.</p>
      <details><summary>Context limits</summary>
        <label>Room for the next reply (tokens)<input type="number" min="4096" max="131072" step="1" bind:value={reserveTokens} required disabled={saving} /></label>
        <label>Recent messages to keep (tokens)<input type="number" min="4096" max="131072" step="1" bind:value={keepRecentTokens} required disabled={saving} /></label>
      </details>
      <button type="submit" disabled={saving}>{saving ? 'Saving…' : 'Save context settings'}</button>
    </form>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>
<style>
  section { border-top: 1px solid var(--border); margin-top: 20px; padding-top: 16px; }
  h3 { font: var(--text-13) var(--font-mono); color: var(--muted); }
  p { font-size: var(--text-13); color: var(--muted); }
  form, details[open] { display: grid; gap: 10px; }
  label { display: flex; align-items: center; justify-content: space-between; gap: 12px; font-size: var(--text-13); }
  .toggle { justify-content: flex-start; }
  input[type="number"] { width: 100px; padding: 5px; color: var(--ink); background: var(--paper); border: 1px solid var(--border); font: var(--text-13) var(--font-mono); }
  button { width: fit-content; padding: 6px 10px; color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); font: inherit; cursor: pointer; }
  summary { cursor: pointer; font-size: var(--text-13); }
</style>

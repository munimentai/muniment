<script>
  import Toggle from '../lib/Toggle.svelte'
  import { onMount } from 'svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
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
<section aria-label="Conversation context">
  <PageHead intro="Summarize older messages to make room. Full history stays saved.">
    {#snippet actions()}{#if ready}<Button type="submit" form="context-settings" disabled={saving}>{saving ? 'Saving…' : 'Save context settings'}</Button>{/if}{/snippet}
  </PageHead>
  {#if ready}
    <form id="context-settings" onsubmit={(event) => { event.preventDefault(); void save() }}>
      <Group heading="Compaction">
        <Row title="Compact automatically" info="When off, a full conversation can stop replies.">
          <Toggle bind:checked={enabled} label="Compact automatically" disabled={saving} />
        </Row>
        <Row title="Room for the next reply" info="Tokens kept free for the model's answer.">
          <input type="number" aria-label="Room for the next reply (tokens)" min="4096" max="131072" step="1" bind:value={reserveTokens} required disabled={saving} />
        </Row>
        <Row title="Recent messages to keep" info="Tokens of the latest messages kept word for word.">
          <input type="number" aria-label="Recent messages to keep (tokens)" min="4096" max="131072" step="1" bind:value={keepRecentTokens} required disabled={saving} />
        </Row>
      </Group>
    </form>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>
<style>
  section { display: grid; gap: var(--page-gap); }
  p { margin: 0; font-size: var(--text-13); color: var(--muted); }
  input[type="number"] { width: 110px; height: 32px; box-sizing: border-box; padding: 0 10px; color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-13) var(--font-mono); }
  input:focus { outline: none; border-color: var(--muted); }
</style>

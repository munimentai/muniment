<script>
  import { onMount, tick } from 'svelte'

  let { statuses, source, names, selected = $bindable(), key = $bindable(), baseUrl = $bindable(), status, pending, active, onsave, ondisconnect, onrefresh, disconnectRetry } = $props()
  let opened = $state(false)
  let control = $state()
  let panel = $state()
  const connected = $derived(statuses.filter((entry) => entry.configured))
  const label = $derived(source === 'ollama' ? 'Local · Ollama' : names[source] ? `${names[source]} key` : 'Connect a model')

  export async function openPanel() {
    opened = true
    await tick()
    panel?.focus()
    void onrefresh()
  }

  function closePanel() {
    opened = false
    key = ''
    baseUrl = ''
    control?.focus()
  }

  function chooseProvider(provider) {
    selected = provider
    key = ''
    baseUrl = ''
  }

  onMount(() => {
    const outside = (event) => {
      const path = event.composedPath()
      if (opened && !path.includes(panel) && !path.includes(control)) closePanel()
    }
    const escape = (event) => {
      if (!opened || event.key !== 'Escape') return
      event.preventDefault()
      event.stopPropagation()
      closePanel()
    }
    document.addEventListener('click', outside, true)
    document.addEventListener('keydown', escape, true)
    return () => {
      document.removeEventListener('click', outside, true)
      document.removeEventListener('keydown', escape, true)
    }
  })
</script>

<div class="model-source">
  <button type="button" bind:this={control} class="model-chip" aria-haspopup="dialog" aria-expanded={opened} aria-controls="model-panel" onclick={() => opened ? closePanel() : openPanel()}>{label}<span aria-hidden="true">⌃</span></button>
  {#if opened}
    <div bind:this={panel} id="model-panel" class="model-panel" role="dialog" aria-label="Model source" tabindex="-1">
      <header><h2>Model source</h2><button type="button" class="close" aria-label="Close model source" onclick={closePanel}>×</button></header>
      <div class="content">
        <section aria-labelledby="connected-models-title">
          <h3 id="connected-models-title">Connected providers</h3>
          {#if connected.length}
            <ul>
              {#each connected as entry (entry.provider)}
                <li><span>{names[entry.provider]}</span><span class="source-tag">{entry.provider === 'ollama' ? 'Local' : 'Key'}</span><button type="button" aria-label={`Disconnect ${names[entry.provider]}`} disabled={active || pending} onclick={() => ondisconnect(entry.provider)}>Disconnect</button></li>
              {/each}
            </ul>
          {:else}<p>No providers connected.</p>{/if}
        </section>
        <form onsubmit={(event) => { event.preventDefault(); onsave() }}>
          <fieldset disabled={active || pending}>
            <legend>Provider</legend>
            <div class="provider-choice">
              {#each Object.entries(names) as [provider, name]}
                <label class="provider-option"><input type="radio" name="provider" value={provider} checked={selected === provider} onchange={() => chooseProvider(provider)}><span>{name}{provider === 'ollama' ? ' (local)' : ''}</span></label>
              {/each}
            </div>
            {#if selected === 'ollama'}
              <label class="field-label" for="provider-base-url">Ollama server URL</label>
              <input id="provider-base-url" type="url" placeholder="http://localhost:11434/v1" autocomplete="url" bind:value={baseUrl}>
              <button class="save" type="submit" disabled={!baseUrl.trim()}>Save Ollama server</button>
            {:else}
              <label class="field-label" for="provider-key">Provider API key</label>
              <input id="provider-key" type="password" autocomplete="off" bind:value={key}>
              <button class="save" type="submit" disabled={!key.trim()}>Save key</button>
            {/if}
          </fieldset>
        </form>
        {#if status}<p class="status" role="status">{status}</p>{/if}
        {#if disconnectRetry}<button type="button" disabled={active || pending} onclick={() => ondisconnect(disconnectRetry)}>Retry disconnect</button>{/if}
        <p class="note">Use your own provider key or a local server.</p>
        <p class="note">No free hosted model exists at the no-account tier.</p>
      </div>
    </div>
  {/if}
</div>

<style>
  .model-source { position: relative; max-width: 100%; }
  button { min-height: 28px; padding: 4px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); cursor: pointer; }
  button:hover:not(:disabled) { border-color: var(--muted); background: var(--faint); }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  button:disabled, fieldset:disabled { color: var(--muted); cursor: default; }
  .model-chip { display: flex; align-items: center; gap: 10px; border-radius: var(--radius-chip); }
  .model-chip[aria-expanded="true"] { border-color: var(--muted); background: var(--faint); }
  .model-chip span { color: var(--muted); }
  .model-panel { position: absolute; z-index: 5; left: 0; bottom: calc(100% + 8px); width: min(360px, calc(100vw - 64px)); max-height: calc(100dvh - 180px); display: flex; flex-direction: column; overflow: hidden; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-panel); box-shadow: var(--shadow-overlay); outline: none; }
  .model-panel:focus-visible { border-color: var(--muted); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 12px 16px; border-bottom: 1px solid var(--border); }
  h2 { margin: 0; font: 600 var(--text-15) var(--font-human); }
  .close { min-width: 28px; padding: 0; font-size: var(--text-17); background: transparent; }
  .content { min-height: 0; overflow-y: auto; padding: 16px; }
  h3, legend { margin: 0 0 8px; padding: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  p { margin: 8px 0; font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  ul { margin: 0; padding: 0; list-style: none; }
  li { display: flex; align-items: center; gap: 8px; padding: 6px 0; font: var(--text-12) var(--font-mono); }
  li button { margin-left: auto; }
  .source-tag { padding: 2px 4px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); }
  form { margin-top: 16px; padding-top: 16px; border-top: 1px solid var(--border); }
  fieldset { min-width: 0; padding: 0; margin: 0; border: 0; }
  .provider-choice { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .provider-option { display: flex; align-items: center; gap: 6px; min-height: 36px; padding: 3px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-mono); cursor: pointer; }
  .provider-option:has(:checked) { border-color: var(--muted); background: var(--faint); }
  fieldset:not(:disabled) .provider-option:hover { border-color: var(--muted); }
  input[type="radio"] { flex: none; width: 24px; height: 24px; margin: 0; accent-color: var(--ink); }
  .field-label { display: block; margin: 14px 0 6px; font: var(--text-13) var(--font-human); }
  input:not([type="radio"]) { box-sizing: border-box; width: 100%; min-height: 34px; padding: 6px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--ink); background: var(--surface); font: var(--text-12) var(--font-mono); }
  .save { display: block; margin: 10px 0 0 auto; }
  .note { color: var(--muted); font-family: var(--font-human); }
  .note:first-of-type { margin-top: 16px; }
  .status { padding-top: 8px; }
</style>

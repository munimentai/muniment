<script>
  // The record panel's first screen for a company that holds no records:
  // what the company record is, and the sources that fill it, each under
  // its own mark. A source opens the import on that source, and the kind
  // follows from the object the user picks there.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import { sourceOptions } from './record-import-state.js'

  let { company = null, onconnect } = $props()
  const sources = sourceOptions()
</script>

<section class="record-connect" aria-label="Connect your data" data-testid="record-connect">
  <h3 class="record-connect-title">Connect your data</h3>
  <p class="record-connect-lede">{company?.name ?? 'This company'} has no records yet. The company record takes over from the systems of record your tools hold. Each source below reads its objects in, keyed on the source's own ids, so a second run updates what changed and never duplicates a row. The agent reads and writes the record from the thread.</p>
  <ul class="record-connect-sources" aria-label="Sources">
    {#each sources as option (option.value)}
      <li>
        <button type="button" class="record-connect-source" onclick={() => onconnect?.(option.value)}>
          <span class="record-connect-mark">
            {#if option.value === 'csv'}
              <LucideIcon name="file-text" size={20} />
            {:else}
              <ProviderLogo provider={`source-${option.value}`} size={20} />
            {/if}
          </span>
          <span class="record-connect-name">{option.label}</span>
          <span class="record-connect-note">{option.note}</span>
        </button>
      </li>
    {/each}
  </ul>
  <p class="record-connect-foot">Every read runs on this machine, and a credential stays in its keychain. Nothing here writes back to a source.</p>
</section>

<style>
  .record-connect { display: grid; align-content: start; gap: 14px; min-height: 0; overflow-y: auto; padding-top: 14px; }
  .record-connect-title { margin: 0; font: 600 var(--text-15)/1.3 var(--font-body); }
  .record-connect-lede, .record-connect-foot { margin: 0; max-width: 62ch; color: var(--ink); font: var(--text-13)/1.5 var(--font-body); }
  .record-connect-foot { color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); }
  .record-connect-sources { margin: 0; padding: 0; list-style: none; display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 8px; }
  .record-connect-source { display: grid; grid-template-columns: 24px minmax(0, 1fr); grid-template-rows: auto auto; align-items: center; column-gap: 10px; width: 100%; min-height: 56px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); text-align: left; cursor: pointer; }
  .record-connect-source:hover { background: var(--faint); }
  .record-connect-source:focus-visible { outline: 2px solid var(--signal); outline-offset: 1px; }
  .record-connect-mark { grid-row: 1 / span 2; display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; color: var(--muted); }
  .record-connect-name { font: 500 var(--text-13)/1.3 var(--font-body); }
  .record-connect-note { color: var(--muted); font: var(--text-12)/1.4 var(--font-mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>

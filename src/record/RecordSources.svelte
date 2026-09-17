<script>
  // The eight sources that read into the record, each under its own mark, in
  // two columns of four. The first screen and the connect screen draw the
  // same grid, so a source is added in one place.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import { sourceOptions } from './record-import-state.js'

  let { onconnect, selected = undefined } = $props()
  const sources = sourceOptions()
</script>

<ul class="record-sources" aria-label="Sources">
  {#each sources as option (option.value)}
    <li>
      <button type="button" class="record-source" class:chosen={selected === option.value} aria-pressed={selected === undefined ? undefined : selected === option.value} onclick={() => onconnect?.(option.value)}>
        <span class="record-source-mark">
          {#if option.value === 'csv'}
            <LucideIcon name="file-text" size={20} />
          {:else}
            <ProviderLogo provider={`source-${option.value}`} size={20} />
          {/if}
        </span>
        <span class="record-source-name">{option.label}</span>
        <span class="record-source-note">{option.note}</span>
      </button>
    </li>
  {/each}
</ul>

<style>
  .record-sources { margin: 0; padding: 0; list-style: none; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .record-source { display: grid; grid-template-columns: 24px minmax(0, 1fr); grid-template-rows: auto auto; align-items: center; column-gap: 10px; width: 100%; min-height: 56px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); text-align: left; cursor: pointer; }
  .record-source:hover { background: var(--faint); }
  .record-source.chosen { background: var(--faint); border-color: var(--muted); }
  .record-source-mark { grid-row: 1 / span 2; display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; color: var(--ink); }
  .record-source-name { font: 500 var(--text-13)/1.3 var(--font-human); }
  .record-source-note { color: var(--muted); font: var(--text-12)/1.4 var(--font-mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>

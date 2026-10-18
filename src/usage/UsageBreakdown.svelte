<script>
  // One breakdown of the range, largest first: each row's name, a bar of its
  // share of the largest row, its amount and its runs.
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import { measure, measureLabel, providerColor, rowLabel } from './usage.js'
  let { rows = [], breakdown = 'model', view = 'tokens' } = $props()
  const peak = $derived(Math.max(0, ...rows.map((row) => measure(row, view))))
</script>

{#if rows.length}
  <ul class="breakdown" aria-label={`Usage by ${breakdown}`}>
    {#each rows as row (row.key)}
      {@const value = measure(row, view)}
      <li>
        <span class="name">{#if row.provider}<ProviderLogo provider={row.provider} size={14} color />{/if}<span>{rowLabel(row, breakdown)}</span></span>
        <span class="bar" aria-hidden="true"><i style:width={peak ? `${(value / peak) * 100}%` : '0%'} style:background={row.provider ? providerColor(row.provider) : 'var(--muted)'}></i></span>
        <span class="value">{measureLabel(value, view)}</span>
        <span class="runs">{row.runs} {row.runs === 1 ? 'run' : 'runs'}</span>
      </li>
    {/each}
  </ul>
{:else}
  <p class="empty">No runs in this range.</p>
{/if}

<style>
  .breakdown { display: grid; margin: 0; padding: 0; list-style: none; }
  li { display: grid; grid-template-columns: minmax(0, 2fr) minmax(60px, 3fr) 72px 64px; align-items: center; gap: 12px; min-height: 32px; border-top: 1px solid var(--border); }
  li:first-child { border-top: 0; }
  .name { display: flex; align-items: center; gap: 8px; min-width: 0; color: var(--ink); font-size: var(--text-13); }
  .name span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar { height: 6px; border-radius: var(--radius-chip); background: var(--faint); overflow: hidden; }
  .bar i { display: block; height: 100%; border-radius: var(--radius-chip); }
  .value, .runs { color: var(--muted); font: var(--text-12) var(--font-mono); text-align: right; font-variant-numeric: tabular-nums; }
  .value { color: var(--ink); }
  .empty { margin: 0; padding: 12px 0; color: var(--muted); font-size: var(--text-13); }
  @container usage (max-width: 520px) { li { grid-template-columns: minmax(0, 1fr) 72px; } .bar, .runs { display: none; } }
</style>

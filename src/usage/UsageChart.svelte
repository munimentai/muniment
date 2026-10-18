<script>
  // The series as stacked bars: one bar per hour or day, one segment per
  // provider in the provider's color, tallest bar at full height. Each bar
  // names its bucket and every provider's amount to assistive technology and
  // on hover.
  import { bucketLabel, measure, measureLabel, providerColor, providerLabel } from './usage.js'
  let { series = [], bucket = 'day', view = 'tokens' } = $props()
  const providers = $derived([...new Set(series.flatMap((entry) => Object.keys(entry.providers ?? {})))].sort())
  const totals = $derived(series.map((entry) => Object.values(entry.providers ?? {}).reduce((sum, amount) => sum + measure(amount, view), 0)))
  const peak = $derived(Math.max(0, ...totals))
  // A tick under every few bars, so labels never crowd.
  const every = $derived(Math.max(1, Math.ceil(series.length / 8)))
  const describe = (entry, total) => {
    const parts = providers.filter((provider) => entry.providers?.[provider]).map((provider) => `${providerLabel(provider)} ${measureLabel(measure(entry.providers[provider], view), view)}`)
    return `${bucketLabel(entry.start_ms, bucket)}: ${parts.length ? parts.join(', ') : 'none'}${parts.length > 1 ? `, ${measureLabel(total, view)} in all` : ''}`
  }
</script>

<figure class="usage-chart" aria-label={view === 'cost' ? 'Estimated cost by provider' : 'Tokens by provider'}>
  <div class="plot" style:--columns={series.length}>
    {#each series as entry, index (entry.start_ms)}
      <div class="column" role="img" aria-label={describe(entry, totals[index])} data-tooltip={describe(entry, totals[index])}>
        <div class="stack" style:height={peak ? `${(totals[index] / peak) * 100}%` : '0%'}>
          {#each providers.filter((provider) => entry.providers?.[provider]) as provider (provider)}
            <span style:flex-grow={measure(entry.providers[provider], view)} style:background={providerColor(provider)}></span>
          {/each}
        </div>
      </div>
    {/each}
  </div>
  <div class="axis" style:--columns={series.length} aria-hidden="true">
    {#each series as entry, index (entry.start_ms)}<span>{index % every === 0 ? bucketLabel(entry.start_ms, bucket) : ''}</span>{/each}
  </div>
  {#if providers.length}
    <figcaption class="legend">
      {#each providers as provider (provider)}<span><i style:background={providerColor(provider)}></i>{providerLabel(provider)}</span>{/each}
      <span class="peak">Peak {measureLabel(peak, view)}</span>
    </figcaption>
  {/if}
</figure>

<style>
  .usage-chart { display: grid; gap: 6px; margin: 0; min-width: 0; }
  .plot, .axis { display: grid; grid-template-columns: repeat(var(--columns), minmax(0, 1fr)); gap: 3px; }
  .plot { height: 160px; align-items: end; padding-bottom: 1px; border-bottom: 1px solid var(--border); }
  .column { position: relative; display: flex; align-items: flex-end; height: 100%; min-width: 0; }
  .column:hover { background: var(--faint); }
  .stack { display: flex; flex-direction: column-reverse; width: 100%; min-height: 0; border-radius: var(--radius-chip); overflow: hidden; }
  .stack span { display: block; flex-basis: 0; min-height: 1px; }
  .axis span { overflow: visible; color: var(--muted); font: var(--text-12) var(--font-human); white-space: nowrap; }
  .legend { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 14px; color: var(--muted); font-size: var(--text-12); }
  .legend span { display: inline-flex; align-items: center; gap: 6px; }
  .legend i { width: 8px; height: 8px; border-radius: var(--radius-chip); }
  .peak { margin-left: auto; font-family: var(--font-mono); }
  .column[data-tooltip]:hover::after { content: attr(data-tooltip); position: absolute; bottom: calc(100% + 6px); left: 50%; z-index: 2; transform: translateX(-50%); padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
</style>

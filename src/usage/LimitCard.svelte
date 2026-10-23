<script>
  // One card in the Limits grid. An account's card names the provider and
  // the account once, then a row per subscription window: its name and
  // reset, a four-step meter of the share left in the fixed meter colors,
  // and the share left. A provider no window meters gets a card with its
  // reason, its address, and the tokens its runs used in the page's range.
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import { compactTokens, meterLevel, rangePhrase, resetLine, unmeteredLabel, unmeteredSubtitle, providerLabel, windowLabel } from './usage.js'
  let { account = null, unmetered = null, range = '7d', machines = [], now = Date.now() } = $props()
  const provider = $derived(account?.provider ?? unmetered?.provider)
  const title = $derived(account ? account.name : unmetered.label || providerLabel(unmetered.provider))
  const subtitle = $derived(account ? account.subtitle : unmeteredSubtitle(unmetered, machines))
  const name = $derived([title, subtitle].filter(Boolean).join(', '))
  const used = $derived(unmetered?.usage ?? null)
  const left = (limit) => Math.max(0, Math.min(100, Number(limit.remaining_percent) || 0))
  const lower = (text) => text.charAt(0).toLowerCase() + text.slice(1)
</script>

<article class="limit-card" aria-label={name}>
  <header>
    <ProviderLogo {provider} size={16} color />
    <h3>{title}</h3>
    {#if unmetered}<span class="slot">{unmeteredLabel(unmetered)}</span>{/if}
  </header>
  {#if subtitle}<p class="account">{subtitle}</p>{/if}
  {#if account}
    <ul class="windows">
      {#each account.windows as limit (`${limit.kind}:${limit.scope}`)}
        {@const remaining = left(limit)}
        {@const level = meterLevel(limit)}
        {@const reset = resetLine(limit, now)}
        <li class="window" data-status={limit.status} data-level={level} aria-label={windowLabel(limit)}>
          <p class="window-name"><span>{windowLabel(limit)}</span><small>{reset}</small></p>
          <span class="meter" data-level={level} role="meter" aria-label={`${windowLabel(limit)} allowance left`} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(remaining)} aria-valuetext={`${level === 0 ? 'Limit reached' : `${Math.round(remaining)}% left`}, ${lower(reset)}`}><i></i><i></i><i></i><i></i></span>
          <span class="left">{level === 0 ? 'Limit' : `${Math.round(remaining)}%`}</span>
        </li>
      {/each}
    </ul>
    {#if account.note}<p class="note">{account.note}</p>{/if}
  {:else if used?.runs}
    <p class="tokens"><b>{compactTokens(used.processed)}</b><span>tokens in {rangePhrase(range)}</span></p>
    <p class="note">{compactTokens(used.input)} in, {compactTokens(used.output)} out across {used.runs} {used.runs === 1 ? 'run' : 'runs'}.</p>
  {:else}
    <p class="note">No runs in this range.</p>
  {/if}
</article>

<style>
  .limit-card { display: grid; align-content: start; gap: 10px; min-width: 0; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  header { display: flex; align-items: center; gap: 8px; min-width: 0; }
  h3 { flex: 1; min-width: 0; margin: 0; overflow: hidden; color: var(--ink); font: 600 var(--text-13) var(--font-human); text-overflow: ellipsis; white-space: nowrap; }
  .slot { flex: none; color: var(--muted); font-size: var(--text-12); }
  .account { margin: -4px 0 0; overflow: hidden; color: var(--muted); font-size: var(--text-12); text-overflow: ellipsis; white-space: nowrap; }
  .windows { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .window { display: grid; grid-template-columns: minmax(0, 1fr) auto 44px; align-items: center; gap: 10px; min-width: 0; min-height: 26px; }
  .window-name { display: grid; min-width: 0; margin: 0; }
  .window-name span, .window-name small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .window-name span { color: var(--ink); font: 500 var(--text-13) var(--font-human); }
  .window-name small { color: var(--muted); font-size: var(--text-12); }
  .meter { display: inline-flex; gap: 4px; padding: 5px 7px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .meter i { width: 16px; height: 7px; border-radius: var(--radius-pill); background: var(--meter-off); }
  .meter[data-level="4"] i { background: var(--meter-4); }
  .meter[data-level="3"] i:nth-child(-n+3) { background: var(--meter-3); }
  .meter[data-level="2"] i:nth-child(-n+2) { background: var(--meter-2); }
  .meter[data-level="1"] i:nth-child(1) { background: var(--meter-1); }
  .left { color: var(--ink); font: 600 var(--text-13) var(--font-human); font-variant-numeric: tabular-nums; text-align: right; }
  [data-level="0"] .left { color: var(--meter-1); }
  .tokens { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 8px; margin: 0; }
  .tokens b { color: var(--ink); font: 600 var(--text-22) var(--font-human); }
  .tokens span, .note { margin: 0; color: var(--muted); font-size: var(--text-12); }
</style>

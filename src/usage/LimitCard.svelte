<script>
  // One account's card: the provider and the account once, then a row per
  // subscription window with the share left, a bar in the provider's color
  // with a line where even spending would be, and the reset. Color marks only
  // a low or spent window.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import { PACE_LABELS, percentLeft, providerColor, resetsIn, windowLabel } from './usage.js'
  let { account, now = Date.now() } = $props()
  const name = $derived([account.name, account.subtitle].filter(Boolean).join(', '))
  const left = (limit) => Math.max(0, Math.min(100, Number(limit.remaining_percent) || 0))
  const evenPace = (limit) => limit.elapsed_percent == null ? null : Math.max(0, Math.min(100, 100 - limit.elapsed_percent))
</script>

<article class="limit-card" aria-label={name}>
  <header>
    <ProviderLogo provider={account.provider} size={16} color />
    <h3>{account.name}</h3>
    {#if account.pace}<span class="pace" data-tooltip={PACE_LABELS[account.pace]} aria-label={PACE_LABELS[account.pace]} role="img"><LucideIcon name={account.pace === 'ahead' ? 'trending-up' : account.pace === 'under' ? 'trending-down' : 'gauge'} size={14} /></span>{/if}
  </header>
  {#if account.subtitle}<p class="account">{account.subtitle}</p>{/if}
  <ul class="windows">
    {#each account.windows as limit (`${limit.kind}:${limit.scope}`)}
      {@const remaining = left(limit)}
      {@const even = evenPace(limit)}
      {@const reset = resetsIn(limit.resets_at_ms, now)}
      <li class="window" data-status={limit.status} aria-label={windowLabel(limit)}>
        <p class="window-name">{windowLabel(limit)}</p>
        <p class="left">
          {#if limit.status === 'limit'}<LucideIcon name="circle-x" size={18} variant="action" /><span>Limit reached</span>
          {:else}{#if limit.status === 'low'}<LucideIcon name="triangle-alert" size={18} variant="action" />{/if}<span>{percentLeft(remaining)}</span>{/if}
        </p>
        <div class="bar" role="meter" aria-label={`${windowLabel(limit)} allowance left`} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(remaining)} aria-valuetext={`${percentLeft(remaining)}${even == null ? '' : `, even pace ${Math.round(even)}%`}`}>
          <span class="fill" style:width={`${remaining}%`} style:background={providerColor(account.provider)}></span>
          {#if even != null}<span class="even" style:left={`${even}%`}></span>{/if}
        </div>
        <p class="reset">{reset || 'Reset time unknown'}</p>
      </li>
    {/each}
  </ul>
  {#if account.note}<p class="note">{account.note}</p>{/if}
</article>

<style>
  .limit-card { display: grid; align-content: start; gap: 10px; min-width: 0; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  header { display: flex; align-items: center; gap: 8px; min-width: 0; }
  h3 { flex: 1; min-width: 0; margin: 0; overflow: hidden; color: var(--ink); font: 600 var(--text-13) var(--font-human); text-overflow: ellipsis; white-space: nowrap; }
  .pace { position: relative; display: inline-flex; color: var(--muted); }
  .account { margin: -4px 0 0; overflow: hidden; color: var(--muted); font-size: var(--text-12); text-overflow: ellipsis; white-space: nowrap; }
  .windows { display: grid; gap: 12px; margin: 0; padding: 0; list-style: none; }
  .window { display: grid; gap: 8px; min-width: 0; }
  .window + .window { padding-top: 12px; border-top: 1px solid var(--border); }
  .window-name { margin: 0; overflow: hidden; color: var(--ink); font: 600 var(--text-12) var(--font-human); text-overflow: ellipsis; white-space: nowrap; }
  .left { display: flex; align-items: center; gap: 6px; margin: -4px 0 0; color: var(--ink); font: 600 var(--text-22) var(--font-human); line-height: 1.3; font-variant-numeric: tabular-nums; }
  [data-status="low"] .left { color: var(--warning); }
  [data-status="limit"] .left { color: var(--danger); }
  .bar { position: relative; height: 8px; border-radius: var(--radius-chip); background: var(--faint); }
  .fill { position: absolute; inset: 0 auto 0 0; border-radius: var(--radius-chip); }
  .even { position: absolute; top: -3px; bottom: -3px; width: 2px; margin-left: -1px; background: var(--ink); opacity: .55; }
  .reset, .note { margin: 0; color: var(--muted); font-size: var(--text-12); }
  [data-tooltip]:hover::after { content: attr(data-tooltip); position: absolute; right: 0; bottom: calc(100% + 6px); z-index: 2; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
</style>

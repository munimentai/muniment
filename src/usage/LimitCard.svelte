<script>
  // One subscription window with its accounts pooled: the share left, a bar
  // in the provider's color with a line where even spending would be, each
  // account as a chip, and the reset. Color marks only a low or spent window.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import { PACE_LABELS, initials, percentLeft, providerColor, providerLabel, resetsIn, windowLabel } from './usage.js'
  let { limit, now = Date.now() } = $props()
  const remaining = $derived(Math.max(0, Math.min(100, Number(limit.remaining_percent) || 0)))
  const evenPace = $derived(limit.elapsed_percent == null ? null : Math.max(0, Math.min(100, 100 - limit.elapsed_percent)))
  const title = $derived(`${providerLabel(limit.provider)} · ${windowLabel(limit)}`)
  const reset = $derived(resetsIn(limit.resets_at_ms, now))
</script>

<article class="limit-card" data-status={limit.status} aria-label={title}>
  <header>
    <ProviderLogo provider={limit.provider} size={16} color />
    <h3>{title}</h3>
    {#if limit.pace}<span class="pace" data-tooltip={PACE_LABELS[limit.pace]} aria-label={PACE_LABELS[limit.pace]} role="img"><LucideIcon name={limit.pace === 'ahead' ? 'trending-up' : limit.pace === 'under' ? 'trending-down' : 'gauge'} size={14} /></span>{/if}
  </header>
  <p class="left">
    {#if limit.status === 'limit'}<LucideIcon name="circle-x" size={18} variant="action" /><span>Limit reached</span>
    {:else}{#if limit.status === 'low'}<LucideIcon name="triangle-alert" size={18} variant="action" />{/if}<span>{percentLeft(remaining)}</span>{/if}
  </p>
  <div class="bar" role="meter" aria-label={`${windowLabel(limit)} allowance left`} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(remaining)} aria-valuetext={`${percentLeft(remaining)}${evenPace == null ? '' : `, even pace ${Math.round(evenPace)}%`}`}>
    <span class="fill" style:width={`${remaining}%`} style:background={providerColor(limit.provider)}></span>
    {#if evenPace != null}<span class="even" style:left={`${evenPace}%`}></span>{/if}
  </div>
  {#if limit.accounts?.length}
    <ul class="accounts" aria-label="Accounts">
      {#each limit.accounts as account (account.id)}
        <li data-tooltip={`${account.label}${account.plan ? ` · ${account.plan}` : ''}`} aria-label={`${account.label}: ${percentLeft(account.remaining_percent)}`}>
          <span class="chip" style:--chip={providerColor(limit.provider)}>{initials(account.label)}</span><span>{Math.round(account.remaining_percent)}%</span>
        </li>
      {/each}
    </ul>
  {/if}
  <p class="reset">{reset || 'Reset time unknown'}</p>
  {#if limit.note}<p class="note">{limit.note}</p>{/if}
</article>

<style>
  .limit-card { display: grid; gap: 10px; min-width: 0; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  header { display: flex; align-items: center; gap: 8px; min-width: 0; }
  h3 { flex: 1; min-width: 0; margin: 0; overflow: hidden; color: var(--ink); font: 600 var(--text-13) var(--font-human); text-overflow: ellipsis; white-space: nowrap; }
  .pace { position: relative; display: inline-flex; color: var(--muted); }
  .left { display: flex; align-items: center; gap: 6px; margin: 0; color: var(--ink); font: 600 var(--text-22) var(--font-human); line-height: 1.3; font-variant-numeric: tabular-nums; }
  [data-status="low"] .left { color: var(--warning); }
  [data-status="limit"] .left { color: var(--danger); }
  .bar { position: relative; height: 8px; border-radius: var(--radius-chip); background: var(--faint); }
  .fill { position: absolute; inset: 0 auto 0 0; border-radius: var(--radius-chip); }
  .even { position: absolute; top: -3px; bottom: -3px; width: 2px; margin-left: -1px; background: var(--ink); opacity: .55; }
  .accounts { display: flex; flex-wrap: wrap; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .accounts li { position: relative; display: inline-flex; align-items: center; gap: 4px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .chip { display: inline-grid; place-items: center; width: 22px; height: 20px; border-radius: var(--radius-control); background: color-mix(in srgb, var(--chip) 22%, var(--surface)); color: var(--ink); font: 600 var(--text-12) var(--font-human); }
  .reset, .note { margin: 0; color: var(--muted); font-size: var(--text-12); }
  [data-tooltip]:hover::after { content: attr(data-tooltip); position: absolute; right: 0; bottom: calc(100% + 6px); z-index: 2; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
  .accounts [data-tooltip]:hover::after { left: 0; right: auto; }
</style>

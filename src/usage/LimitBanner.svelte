<script>
  // A line over the composer only when a subscription window is spent or
  // nearly spent: which window, and when it resets. View usage opens the page.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { percentLeft, providerLabel, resetsIn, windowLabel } from './usage.js'
  let { limit, now = Date.now(), onview = null } = $props()
  const spent = $derived(limit.status === 'limit')
  const reset = $derived(resetsIn(limit.resets_at_ms, now))
  const line = $derived(`${providerLabel(limit.provider)} ${windowLabel(limit).toLowerCase()} ${spent ? 'limit reached' : percentLeft(limit.remaining_percent)}.${reset ? ` ${reset}.` : ''}`)
</script>

<section class="limit-banner" data-status={limit.status} aria-live="polite" aria-label="Usage limit">
  <LucideIcon name={spent ? 'circle-x' : 'triangle-alert'} size={14} variant="action" />
  <p>{line}</p>
  {#if onview}<button type="button" onclick={onview}>View usage</button>{/if}
</section>

<style>
  .limit-banner { display: flex; align-items: center; gap: 8px; margin: 0 0 6px; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--warning); font: var(--text-12) var(--font-human); }
  .limit-banner[data-status="limit"] { color: var(--danger); }
  p { flex: 1; min-width: 0; margin: 0; color: var(--ink); }
  button { flex: none; min-height: 24px; padding: 2px 8px; border: 0; border-radius: var(--radius-control); background: var(--faint); color: var(--ink); font: inherit; cursor: pointer; }
</style>

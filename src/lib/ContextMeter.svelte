<script>
  import { contextHoverLabel, contextLevel, contextPercentLabel, contextShare } from './context-usage.js'
  // The composer band's context control: a ring and a percent in mono. It
  // opens the Context panel.
  let { usage, open = false, compacting = false, onclick } = $props()
  // The ring circumference for a radius of 6.
  const CIRCUMFERENCE = 37.699
  const level = $derived(contextLevel(usage))
  const filled = $derived(contextShare(usage) * CIRCUMFERENCE)
  const label = $derived(compacting ? 'Compacting context' : `Context, ${contextHoverLabel(usage)}`)
</script>
<button type="button" class="context-trigger" data-level={level} class:compacting aria-haspopup="dialog" aria-expanded={open} aria-label={label} data-tooltip={compacting ? 'Compacting context' : contextHoverLabel(usage)} {onclick}>
  <svg class="ring" viewBox="0 0 16 16" aria-hidden="true">
    <circle class="track" cx="8" cy="8" r="6" />
    <circle class="fill" cx="8" cy="8" r="6" stroke-dasharray={`${filled} ${CIRCUMFERENCE}`} />
  </svg>
  {#if !compacting}<span>{contextPercentLabel(usage)}</span>{/if}
</button>
<style>
  .context-trigger { position: relative; flex: none; display: inline-flex; align-items: center; gap: 5px; min-width: 24px; min-height: 24px; padding: 3px 4px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; cursor: pointer; }
  .context-trigger:hover, .context-trigger[aria-expanded="true"] { background: var(--faint); }
  .ring { width: 14px; height: 14px; transform: rotate(-90deg); }
  circle { fill: none; stroke-width: 2; }
  .track { stroke: var(--border); }
  .fill { stroke: var(--muted); }
  [data-level="caution"] { color: var(--ochre); }
  [data-level="caution"] .fill { stroke: var(--ochre); }
  [data-level="unknown"] .track { stroke: var(--muted); stroke-dasharray: 2 2; }
  .compacting .ring { animation: turn 1.1s linear infinite; }
  @keyframes turn { to { transform: rotate(270deg); } }
  :global(:root[data-motion="reduce"]) .compacting .ring { animation: none; }
  .context-trigger[data-tooltip]::after { content: attr(data-tooltip); position: absolute; left: 0; bottom: calc(100% + 6px); padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .context-trigger[data-tooltip]:hover::after, .context-trigger[data-tooltip]:focus-visible::after { opacity: 1; transition-delay: 350ms; }
  .context-trigger[aria-expanded="true"]::after { display: none; }
</style>

<script>
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { executionTime } from '../lib/execution-time.js'
  import { runLineLabel, runLineParts, runTimeline } from './run-trust.js'

  // One line that opens a turn: where it ran, on what, in which mode, how
  // long, how many tokens and what it would cost at API prices. It opens to
  // the turn's tool calls in order.
  let { run, machine = null, modes = [], live = false, liveMode = null, onreview = null } = $props()

  let now = $state(Date.now())
  $effect(() => {
    if (!live) return
    const timer = setInterval(() => { now = Date.now() }, 1000)
    return () => clearInterval(timer)
  })
  const started = $derived(run?.startedAt ? new Date(run.startedAt).getTime() : NaN)
  const elapsed = $derived(live && Number.isFinite(started) ? executionTime((now - started) / 1000) : null)
  const parts = $derived(runLineParts(run, { machine, modes, elapsed, liveMode }))
  const timeline = $derived(runTimeline(run?.toolActivity ?? [], live))
  const items = $derived([parts.machine, parts.model, parts.mode, parts.time, parts.tokens].filter(Boolean))
  const expandable = $derived(timeline.length > 0 || !!onreview)
</script>

{#snippet line()}
  {#each items as item, index}{#if index}<span class="separator" aria-hidden="true">·</span>{/if}<span>{item}</span>{/each}{#if parts.cost}{#if items.length}<span class="separator" aria-hidden="true">·</span>{/if}<span>{parts.cost} API estimate</span>{/if}
{/snippet}

{#if items.length || parts.cost}
  {#if expandable}
    <details class="run-line" data-testid="run-line">
      <summary aria-label={`Run: ${runLineLabel(parts)}`}><span class="marker" aria-hidden="true"><LucideIcon name="chevron-right" size={12} variant="action" /></span>{@render line()}</summary>
      {#if timeline.length}
        <ol class="timeline" aria-label="Tool calls">
          {#each timeline as entry (entry.id)}
            <li><LucideIcon name={entry.icon} size={14} /><span class="label">{entry.label}</span><span class="state" class:failed={entry.state === 'Failed'}>{entry.state}</span>{#if entry.duration}<span class="duration">{entry.duration}</span>{/if}</li>
          {/each}
        </ol>
      {:else}
        <p class="empty">No tool calls.</p>
      {/if}
      {#if onreview}<button type="button" class="review" onclick={onreview}>Review changes</button>{/if}
    </details>
  {:else}
    <p class="run-line plain" data-testid="run-line" aria-label={`Run: ${runLineLabel(parts)}`}>{@render line()}</p>
  {/if}
{/if}

<style>
  .run-line { margin: 0 0 10px; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-mono); font-variant-numeric: tabular-nums; }
  summary, .plain { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-height: 24px; overflow-wrap: anywhere; }
  summary { width: max-content; max-width: 100%; margin-left: -8px; padding: 2px 8px; border-radius: var(--radius-control); cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary:hover { background: var(--faint); color: var(--ink); }
  .marker { display: inline-flex; transition: transform 120ms ease; }
  details[open] .marker { transform: rotate(90deg); }
  .separator { opacity: .7; }
  .timeline { display: grid; gap: 4px; margin: 6px 0 0; padding: 0 0 0 4px; list-style: none; }
  .timeline li { display: grid; grid-template-columns: 14px minmax(0, 1fr) auto auto; align-items: center; gap: 8px; min-height: 24px; }
  .label { overflow: hidden; color: var(--ink); text-overflow: ellipsis; white-space: nowrap; }
  .state.failed { color: var(--danger); }
  .duration { min-width: 40px; text-align: right; }
  .empty { margin: 6px 0 0; }
  .review { min-height: 28px; margin-top: 8px; padding: 4px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-12) var(--font-human); cursor: pointer; }
  .review:hover { background: var(--faint); }
  @media (prefers-reduced-motion: reduce) { .marker { transition: none; } }
</style>

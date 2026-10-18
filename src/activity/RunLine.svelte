<script>
  import ToolCall from './ToolCall.svelte'
  import { executionTime } from '../lib/execution-time.js'
  import { runLineLabel, runLineParts, runTimeline } from './run-trust.js'

  // One line that opens a turn: where it ran, on what, in which mode, how
  // long, how many tokens and what it would cost at API prices. It opens to
  // the turn's tool calls in order. A long transcript holds many of these,
  // so the closed line is one element and its parts render only once open.
  let { run, machine = null, modes = [], live = false, liveMode = null, sources = {}, onreview = null } = $props()

  let now = $state(Date.now())
  let open = $state(false)
  $effect(() => {
    if (!live) return
    const timer = setInterval(() => { now = Date.now() }, 1000)
    return () => clearInterval(timer)
  })
  const started = $derived(run?.startedAt ? new Date(run.startedAt).getTime() : NaN)
  const elapsed = $derived(live && Number.isFinite(started) ? executionTime((now - started) / 1000) : null)
  const parts = $derived(runLineParts(run, { machine, modes, elapsed, liveMode, sources }))
  const timeline = $derived(open ? runTimeline(run?.toolActivity ?? [], live) : [])
  const text = $derived([parts.machine, parts.model, parts.mode, parts.time, parts.tokens, parts.cost && `${parts.cost} API estimate`].filter(Boolean).join(' · '))
  const expandable = $derived((run?.toolActivity?.length ?? 0) > 0)
</script>

{#if text}
  {#if expandable}
    <details class="run-line" data-testid="run-line" bind:open>
      <summary aria-label={`Run: ${runLineLabel(parts)}`}>{text}</summary>
      {#if open}
        <ol class="timeline" aria-label="Tool calls">
          {#each timeline as entry (entry.id)}
            <li><ToolCall presentation={entry.presentation} label={entry.label} running={entry.running} result={entry.result || (entry.state === 'Completed' ? '' : entry.state)} failed={entry.failed} iconSize={14} />{#if entry.duration}<span class="duration">{entry.duration}</span>{/if}</li>
          {/each}
        </ol>
        {#if onreview}<button type="button" class="review" onclick={onreview}>Review changes</button>{/if}
      {/if}
    </details>
  {:else}
    <p class="run-line plain" data-testid="run-line" aria-label={`Run: ${runLineLabel(parts)}`}>{text}</p>
  {/if}
{/if}

<style>
  .run-line { margin: 0 0 10px; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  summary, .plain { min-height: 24px; padding: 2px 0; }
  summary { display: block; width: max-content; max-width: 100%; margin-left: -8px; padding: 2px 8px; border-radius: var(--radius-control); cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  /* The chevron of the receipt line, drawn without an element. */
  summary::before { content: ''; display: inline-block; width: 5px; height: 5px; margin-right: 8px; border-right: 1px solid currentColor; border-bottom: 1px solid currentColor; transform: rotate(-45deg); transition: transform 120ms ease; vertical-align: 1px; }
  details[open] > summary::before { transform: rotate(45deg); }
  summary:hover { background: var(--faint); color: var(--ink); }
  .timeline { display: grid; gap: 4px; margin: 6px 0 0; padding: 0 0 0 4px; list-style: none; }
  .timeline li { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 8px; min-height: 24px; }
  .timeline li :global(.tool-label) { color: var(--ink); }
  .duration { min-width: 40px; text-align: right; }
  .review { min-height: 28px; margin-top: 8px; padding: 4px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-12) var(--font-human); cursor: pointer; }
  .review:hover { background: var(--faint); }
</style>

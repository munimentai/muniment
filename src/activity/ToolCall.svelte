<script>
  // One tool call as core presents it. `line` is its row in the transcript:
  // icon, headline or the words while it runs, how it ran
  // when a script made it, and how it ended. `card` leads an approval with the headline, the reason and the
  // details. `detail` is an open row's body: the details and its state. The
  // raw input and output stay behind Show details in both.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { headlineParts, presentationOf } from './tool-call.js'

  let { presentation = null, variant = 'line', label = '', running = false, result = '', failed = false, state = '', reason = '', input = '', raw = undefined, via = '', iconSize = 16 } = $props()
  const shown = $derived(presentationOf({ presentation }))
  const parts = $derived(headlineParts(label || shown.headline))
  const details = $derived((shown.details ?? []).filter((detail) => detail?.label && detail?.value))
  const hasRaw = $derived(!!input || !!raw)
</script>

{#snippet headline()}
  {#each parts as part, index (index)}{#if part.code}<code>{part.text}</code>{:else}{part.text}{/if}{/each}
{/snippet}

{#snippet detailList()}
  {#if details.length}
    <dl class="tool-details">
      {#each details as detail, index (index)}<div><dt>{detail.label}</dt><dd class:mono={detail.mono}>{detail.value}</dd></div>{/each}
    </dl>
  {/if}
{/snippet}

{#snippet rawDetails()}
  {#if hasRaw}
    <details class="tool-raw">
      <summary>Show details</summary>
      {#if input}<strong>Input</strong><pre>{input}</pre>{/if}
      {#if raw}{@render raw()}{/if}
    </details>
  {/if}
{/snippet}

{#if variant === 'card'}
  <div class="tool-call-card" data-tool-kind={shown.kind}>
    <p class="tool-title"><LucideIcon name={shown.icon} size={iconSize} /><span>{@render headline()}</span></p>
    {#if reason}<p class="tool-reason">{reason}</p>{/if}
    {@render detailList()}
    {@render rawDetails()}
  </div>
{:else if variant === 'detail'}
  <div class="tool-body">
    {@render detailList()}
    {#if state}<p class="tool-state">{state}</p>{/if}
    {@render rawDetails()}
  </div>
{:else}
  <span class="tool-line" data-tool-kind={shown.kind}>
    <LucideIcon name={shown.icon} size={iconSize} />
    <span class="tool-label" class:active-sheen={running}>{@render headline()}</span>
    {#if via}<span class="tool-via">{via}</span>{/if}
    {#if result}<span class="tool-result" class:failed>{result}</span>{:else if state === 'Failed' || state === 'Interrupted'}<span class="tool-result" class:failed={state === 'Failed'}>{state}</span>{/if}
  </span>
{/if}

<style>
  .tool-line { display: inline-flex; align-items: center; gap: 8px; min-width: 0; max-width: 100%; }
  .tool-line :global(svg) { flex: none; }
  .tool-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .tool-result { flex: none; max-width: 45%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); font-size: var(--text-12); }
  .tool-result.failed { color: var(--danger); }
  .tool-via { flex: none; color: var(--muted); font-size: var(--text-12); }
  code { font-family: var(--font-mono); }
  .tool-call-card { display: grid; gap: 6px; min-width: 0; }
  .tool-title { display: flex; align-items: flex-start; gap: 8px; margin: 0; color: var(--ink); font: 500 var(--text-15)/1.4 var(--font-human); overflow-wrap: anywhere; }
  .tool-title :global(svg) { flex: none; margin-top: 3px; }
  .tool-reason { margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.4; overflow-wrap: anywhere; }
  .tool-details { display: grid; gap: 4px; margin: 0; padding: 8px 10px; border-radius: var(--radius-control); background: var(--faint); font-size: var(--text-12); }
  .tool-details div { display: grid; grid-template-columns: minmax(72px, max-content) minmax(0, 1fr); gap: 12px; }
  dt { color: var(--muted); }
  dd { margin: 0; color: var(--ink); white-space: pre-wrap; overflow-wrap: anywhere; }
  dd.mono { font-family: var(--font-mono); }
  .tool-body { display: grid; gap: 8px; }
  .tool-state { margin: 0; }
  .tool-raw summary { display: inline-flex; align-items: center; min-height: 24px; padding: 2px 6px; margin-left: -6px; border-radius: var(--radius-control); color: var(--muted); font-size: var(--text-12); cursor: pointer; list-style: none; }
  .tool-raw summary::-webkit-details-marker { display: none; }
  .tool-raw summary:hover, .tool-raw summary:focus-visible { background: var(--faint); color: var(--ink); }
  .tool-raw strong { display: block; margin-top: 6px; font-weight: 500; font-size: var(--text-12); }
  .tool-raw :global(pre) { font-family: var(--font-mono); font-size: var(--text-12); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 320px; overflow: auto; margin: 4px 0 8px; }
  .active-sheen { background: linear-gradient(100deg, var(--muted) 35%, var(--ink) 50%, var(--muted) 65%); background-size: 250% 100%; background-clip: text; -webkit-background-clip: text; color: transparent; animation: tool-sheen 2.4s linear infinite; }
  @keyframes tool-sheen { from { background-position: 140% 0; } to { background-position: -40% 0; } }
  :global(:root[data-motion="reduce"]) .active-sheen { animation: none; background: none; color: var(--muted); }
  @media (forced-colors: active) { .active-sheen { background: none; color: CanvasText; } }
</style>

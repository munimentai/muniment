<script>
  import { onMount, untrack } from 'svelte'
  import { stageWord } from './chat-state.js'
  import { thinkingSettle } from './thinking-transition.js'
  import { graphPaths } from './graph-mark.js'
  import { REST_POSE, paint, subscribe } from './ring-motion.js'

  // The mark in flight: its motion and color stay, and the word tracks what the
  // run does. A word holds at least this long, so a fast tool never flickers.
  const HOLD_MS = 800
  // The chat reduction retains clear connections at 20px.
  const SIZE = 20
  const rest = graphPaths(SIZE)
  let outline
  let { stage = 'routing' } = $props()
  let shown = $state(stageWord(stage))
  let shownAt = Date.now()
  let group
  let body
  let accent

  $effect(() => {
    const next = stageWord(stage)
    return untrack(() => {
      if (next === shown) return
      const wait = HOLD_MS - (Date.now() - shownAt)
      if (wait <= 0) {
        shown = next
        shownAt = Date.now()
        return
      }
      const timer = setTimeout(() => { shown = next; shownAt = Date.now() }, wait)
      return () => clearTimeout(timer)
    })
  })

  // Every mark on screen follows the one organism, so two runs in flight
  // breathe and turn together. Under reduced motion the still pose stays.
  onMount(() => {
    const parts = { group, body, outline, accent }
    paint(REST_POSE, parts)
    return subscribe((pose) => paint(pose, parts))
  })
</script>

<span class="thinking" out:thinkingSettle|global><svg width={SIZE} height={SIZE} viewBox="0 0 48 48" aria-label={shown}><g bind:this={group}><path class="body" bind:this={body} d={rest.edges} stroke-width={rest.width} /><path class="body" bind:this={outline} d={rest.outline} stroke-width={rest.outlineWidth} /><path class="accent" bind:this={accent} aria-hidden="true" /></g></svg><span>{shown}</span></span>

<style>
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thinking svg { overflow: visible; }
  .thinking .body { fill: none; stroke: var(--signal); stroke-linecap: round; stroke-linejoin: round; }
  .thinking .accent { fill: none; stroke: var(--signal); stroke-linecap: round; opacity: 0; }
</style>

<script>
  import { untrack } from 'svelte'
  import { stageWord } from './chat-state.js'
  import { thinkingSettle } from './thinking-transition.js'

  // The mark in flight: its motion and color stay, and the word tracks what the
  // run does. A word holds at least this long, so a fast tool never flickers.
  const HOLD_MS = 800
  let { stage = 'routing', d } = $props()
  let shown = $state(stageWord(stage))
  let shownAt = Date.now()

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
</script>

<span class="thinking" out:thinkingSettle|global><svg width="17" height="17" viewBox="0 0 48 48" aria-label={shown}><path {d} fill-rule="evenodd" /></svg><span>{shown}</span></span>

<style>
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thinking path { fill: var(--signal); animation: breathe 1.8s ease-in-out infinite; }
  @keyframes breathe { 50% { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) {
    .thinking path { animation: none; }
  }
</style>

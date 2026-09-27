<script>
  import { untrack } from 'svelte'
  import { stageWord } from './chat-state.js'
  import { thinkingSettle } from './thinking-transition.js'
  import GraphMark from './GraphMark.svelte'

  // The geometric mark stays still, and the word tracks what the
  // run does. A word holds at least this long, so a fast tool never flickers.
  const HOLD_MS = 800
  // The compact symbol keeps the folded ear at 20px.
  const SIZE = 20
  let { stage = 'routing' } = $props()
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

<span class="thinking" out:thinkingSettle|global><GraphMark size={SIZE} /><span>{shown}</span></span>

<style>
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
</style>

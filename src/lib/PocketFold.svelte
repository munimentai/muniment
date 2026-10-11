<script>
  import { FOLD_BODY, FOLD_EAR, FOLD_VIEWBOX } from './pocket-fold.js'
  // The still pocket-fold symbol: the body in ink and the ear in signal.
  let { size = 96 } = $props()
</script>

<svg width={size} height={Math.round(size * 499 / 566)} viewBox={FOLD_VIEWBOX} aria-hidden="true">
  <path class="body" d={FOLD_BODY} />
  <path class="ear" d={FOLD_EAR} />
</svg>

<style>
  .body { fill: var(--ink); }
  .ear { fill: var(--signal); }
</style>

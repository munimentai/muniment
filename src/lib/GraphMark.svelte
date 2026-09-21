<script>
  import { graphPaths } from './graph-mark.js'
  let { size = 160 } = $props()
  const graph = $derived(graphPaths(size))
</script>

<svg width={size} height={size} viewBox="0 0 48 48" aria-hidden="true">
  <g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
    <path d={graph.edges} stroke-width={graph.width} />
    {#if graph.outlineWidth}<path d={graph.outline} stroke-width={graph.outlineWidth} />{/if}
    {#if size >= 96}{#each graph.points as point}<circle cx={point[0]} cy={point[1]} r="0.2" fill="currentColor" stroke="none" />{/each}{/if}
  </g>
</svg>

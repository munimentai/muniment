<script>
  import { panelScroll } from '../panel-scroll.js'
  // The body of every full panel. Settings pages and the Agents, Artifacts and
  // Projects catalogs scroll in it with one padding and one gap between
  // sections, so no panel drifts in density. A measured page keeps its rows
  // at a readable width.
  let { children, measure = false, ...rest } = $props()
</script>

<div class="panel-page" use:panelScroll {...rest}>
  <div class="panel-page-body" class:measure>{@render children?.()}</div>
</div>

<style>
  .panel-page { flex: 1; min-height: 0; overflow-y: auto; padding: var(--page-pad); }
  .panel-page-body { display: grid; gap: var(--page-gap); align-content: start; min-width: 0; }
  .measure { max-width: 760px; }
</style>

<script>
  import { panelScroll } from '../panel-scroll.js'
  // The body of every full panel. Settings pages and the Agents, Artifacts and
  // Projects catalogs scroll in it with one padding and one gap between
  // sections, so no panel drifts in density. The body takes the panel's full
  // width, and each section flows its own content into it.
  let { children, ...rest } = $props()
</script>

<div class="panel-page" use:panelScroll {...rest}>
  <div class="panel-page-body">{@render children?.()}</div>
</div>

<style>
  .panel-page { flex: 1; min-height: 0; overflow-y: auto; padding: var(--page-pad); }
  .panel-page-body { display: grid; gap: var(--page-gap); align-content: start; min-width: 0; }
</style>

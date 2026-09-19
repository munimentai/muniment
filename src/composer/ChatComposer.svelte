<script>
  // Both regular threads and agent threads render this same composer instance.
  // Snippets retain the shared chat controller bindings and feature controls.
  let { element = $bindable(), children } = $props()
</script>
<div class="composer" bind:this={element}>{@render children()}</div>
<style>
  .composer { grid-area: 1 / 1; align-self: end; z-index: 1; position: relative; width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .composer:focus-within { border-color: var(--muted); }
</style>

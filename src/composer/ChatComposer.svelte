<script>
  // Both regular threads and agent threads render this same composer instance.
  // Snippets retain the shared chat controller bindings and feature controls.
  // The composer is a size container, so a narrow one keeps its menu buttons to icons.
  let { element = $bindable(), children } = $props()
</script>
<div class="composer" bind:this={element}>{@render children()}</div>
<style>
  .composer { container: composer / inline-size; grid-area: 1 / 1; align-self: end; z-index: 1; position: relative; width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .composer:focus-within { border-color: var(--muted); }
</style>

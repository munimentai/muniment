<script>
  // The grid every catalog lays its cards in: as many 220px columns as fit.
  let { children, ...rest } = $props()
</script>

<div class="card-grid" {...rest}>{@render children?.()}</div>

<style>
  .card-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }
</style>

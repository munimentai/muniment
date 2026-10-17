<script>
  // The grid every catalog lays its cards in: as many `min` px columns as fit.
  // `rows` gives every row one fixed height, so a card's content never moves the grid.
  let { children, min = 220, rows = null, ...rest } = $props()
</script>

<div class="card-grid" style:--card-min="{min}px" style:grid-auto-rows={rows ? `${rows}px` : null} {...rest}>{@render children?.()}</div>

<style>
  .card-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(var(--card-min), 100%), 1fr)); gap: 12px; }
</style>

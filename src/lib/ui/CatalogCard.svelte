<script>
  // One card in a CardGrid: a button that opens the item, with an icon, the
  // name in strong, a span of support and a small note. `menu` puts the
  // item's actions in the card's top right corner.
  let { menu, children, ...rest } = $props()
</script>

<div class="catalog-card">
  {@render menu?.()}
  <button type="button" class="card" {...rest}>{@render children?.()}</button>
</div>

<style>
  .catalog-card { position: relative; display: flex; min-width: 0; }
  .catalog-card > :global(.catalog-actions) { position: absolute; top: 8px; right: 8px; }
  .card { display: flex; flex-direction: column; align-items: flex-start; gap: 10px; width: 100%; min-width: 0; padding: 32px 16px 16px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; overflow-wrap: anywhere; cursor: pointer; }
  .card:hover:not(:disabled) { background: var(--faint); }
  .card:disabled { cursor: default; }
  .card > :global(strong) { font-size: var(--text-13); font-weight: 600; }
  .card > :global(:is(span, small)) { color: var(--muted); }
  .card > :global(span) { display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
</style>

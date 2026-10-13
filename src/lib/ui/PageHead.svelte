<script>
  // A page's first row: the intro at the left and the page's actions at the
  // right. Below 560px an action keeps its icon and hides its .action-label.
  // `lead` puts a control, such as Back, before the intro. A narrow page moves
  // the actions below the intro rather than squeezing the intro.
  let { intro = '', lead, actions } = $props()
</script>

<div class="page-head">
  {@render lead?.()}
  <p class="settings-intro">{intro}</p>
  {#if actions}<div class="page-actions">{@render actions()}</div>{/if}
</div>

<style>
  .page-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 16px; min-height: 32px; container-type: inline-size; }
  .page-head > p { flex: 1 1 280px; min-width: 0; margin: 0; }
  .page-actions { display: flex; flex: none; align-items: center; gap: 8px; }
  @container (max-width: 560px) { .page-actions :global(.action-label) { display: none; } }
</style>

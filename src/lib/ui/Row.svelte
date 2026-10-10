<script>
  // One row of a Group card: the title and its support line at the left and
  // the control at the right. `lead` puts a mark beside the text, `icon` puts
  // a logo inline before the title,
  // `meta` adds a mono note after it, `detail` adds content under the text,
  // and `stacked` puts a wide control under the text. A narrow row wraps its
  // control under the text.
  let { title = '', meta = '', description = '', label = '', stacked = false, lead, icon, detail, children } = $props()
</script>

<div class="row" class:stacked role={label ? 'group' : undefined} aria-label={label || undefined}>
  {#if lead}<span class="row-lead">{@render lead()}</span>{/if}
  <div class="row-text">
    {#if title}<strong>{@render icon?.()}{title}{#if meta}<span class="row-meta">{meta}</span>{/if}</strong>{/if}
    {#if description}<p>{description}</p>{/if}
    {@render detail?.()}
  </div>
  {#if children}<div class="row-control">{@render children()}</div>{/if}
</div>

<style>
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 16px; min-width: 0; min-height: 44px; padding: 10px 0; box-sizing: border-box; }
  .row-lead { flex: none; display: inline-flex; align-items: center; }
  .row-text { flex: 1 1 200px; display: grid; gap: 2px; min-width: 0; }
  .row-text > strong { display: flex; align-items: center; gap: 8px; min-width: 0; font-size: var(--text-13); font-weight: 600; overflow-wrap: anywhere; }
  .row-meta { color: var(--muted); font: var(--weight-regular) var(--text-12) var(--font-mono); }
  .row-text > p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .row-control { flex: none; display: flex; align-items: center; gap: 6px; min-width: 0; }
  .stacked .row-control { flex: 1 1 100%; }
</style>

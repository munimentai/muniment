<script>
  // One section of a page: a semibold heading with its support line and
  // actions, then its rows in one bordered card. Every Settings page builds
  // its sections from Group and Row, so the pages share one design. A plain
  // group keeps the heading and drops the card, for a grid of cards.
  let { heading = '', description = '', label = '', count = null, level = 4, plain = false, lead, actions, children } = $props()
  const id = $props.id()
</script>

<section class="group" aria-labelledby={heading ? `${id}-heading` : undefined} aria-label={heading ? undefined : label || undefined}>
  {#if heading || actions}
    <div class="group-head">
      {#if heading}<svelte:element this={`h${level}`} id={`${id}-heading`} class="settings-heading group-heading">{@render lead?.()}{heading}{#if count !== null}<span class="group-count">{count}</span>{/if}</svelte:element>{/if}
      {#if actions}<div class="group-actions">{@render actions()}</div>{/if}
    </div>
  {/if}
  {#if description}<p class="group-description">{description}</p>{/if}
  {#if plain}{@render children?.()}{:else}<div class="group-card">{@render children?.()}</div>{/if}
</section>

<style>
  .group { display: grid; gap: 10px; min-width: 0; }
  .group-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 24px; }
  .group-heading { display: flex; align-items: center; gap: 8px; min-width: 0; margin: 0; }
  .group-count { color: var(--muted); font: var(--weight-regular) var(--text-12) var(--font-mono); }
  .group-actions { display: flex; flex: none; align-items: center; gap: 6px; }
  .group-description { margin: -4px 0 0; color: var(--muted); font-size: var(--text-13); }
  .group-card { display: grid; min-width: 0; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .group-card > :global(* + *) { border-top: 1px solid var(--border); }
  .group-card > :global(p) { margin: 0; padding: 12px 0; color: var(--muted); font-size: var(--text-13); }
</style>

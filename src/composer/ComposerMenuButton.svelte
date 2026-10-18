<script>
  // The one button behind each composer menu: machine, model, thinking level
  // and usage, named by `menu`. At rest it is muted with no fill. Hovered or
  // open, it takes the hover fill in ink. `chevrons` adds the closed and open
  // arrows. A narrow composer shows the icon alone, `compact` when given, and
  // the accessible name and a hover label keep the full label.
  import LucideIcon from '../lib/LucideIcon.svelte'
  let { menu, open = false, label, text, icon, compact = null, chevrons = false, haspopup = 'menu', element = $bindable(), ...rest } = $props()
</script>

<button type="button" class="composer-menu-button" class:model-chip={menu === 'model'} data-menu={menu} data-open={open ? 'true' : undefined} data-tooltip={label} aria-haspopup={haspopup} aria-expanded={open} aria-label={label} bind:this={element} {...rest}>
  <span class="icon" class:wide={!!compact}>{@render icon()}</span>
  {#if compact}<span class="icon narrow">{@render compact()}</span>{/if}
  <span class="label">{text}</span>
  {#if chevrons}<span class="chevrons"><LucideIcon name={open ? 'chevrons-down-up' : 'chevrons-up-down'} variant="action" size={12} /></span>{/if}
</button>

<style>
  /* The composer controls share the plus button's box: 4px padding, a 24px minimum, the control radius. */
  .composer-menu-button { position: relative; flex: none; display: inline-flex; align-items: center; gap: 5px; min-width: 24px; min-height: 24px; max-width: 220px; padding: 3px 4px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-12) var(--font-human); white-space: nowrap; cursor: pointer; }
  .composer-menu-button:hover:not(:disabled), .composer-menu-button[data-open="true"] { color: var(--ink); background: var(--faint); }
  .composer-menu-button:disabled { cursor: default; }
  .icon, .chevrons { flex: none; display: inline-flex; }
  .icon :global(.lucide) { color: inherit; }
  .narrow { display: none; }
  /* The label's line box holds the descenders the clip would take off a g or a p. */
  .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; line-height: 16px; }
  /* The hover label shows only while the button shows its icon alone. */
  .composer-menu-button::after { content: attr(data-tooltip); position: absolute; left: 0; bottom: calc(100% + 6px); display: none; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .composer-menu-button:hover::after, .composer-menu-button:focus-visible::after { opacity: 1; transition-delay: 350ms; }
  /* A narrow composer keeps each control to its icon. */
  @container composer (max-width: 480px) {
    .label, .chevrons, .wide { display: none; }
    .narrow { display: inline-flex; }
    .composer-menu-button::after { display: block; }
    .composer-menu-button[data-open="true"]::after { display: none; }
  }
</style>

<script>
  import LucideIcon from '../LucideIcon.svelte'
  let { children, icon, iconOnly = false, variant = 'default', ...props } = $props()
</script>
<button data-ui-button type="button" {...props} class:icon-only={iconOnly} data-variant={variant}>
  {#if icon}<LucideIcon name={icon} size={16} variant="action" />{/if}
  {@render children?.()}
</button>
<style>
  button { display: inline-flex; align-items: center; justify-content: center; flex: none; gap: 8px; min-height: 32px; min-width: 32px; box-sizing: border-box; padding: 6px 10px; font: var(--text-13) var(--font-human); line-height: 1.4; color: var(--ink); background: var(--faint); border: 1px solid transparent; border-radius: var(--radius-control); cursor: pointer; }
  button[data-variant="outline"] { border-color: var(--border); background: var(--surface); }
  button:hover:not(:disabled), button[aria-expanded="true"] { border-color: var(--muted); }
  button[aria-pressed="true"] { background: var(--faint); border-color: var(--muted); }
  button:focus-visible { background: var(--faint); color: var(--ink); }
  button:disabled { opacity: .5; cursor: default; }
  .icon-only { padding: 6px; }
</style>

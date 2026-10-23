<script>
  import LucideIcon from '../LucideIcon.svelte'
  // `primary` fills the button with the scheme accent: the one action a view
  // asks for, such as New agent, Run now, Connect or Save. A submit button is
  // primary unless it names another variant. `danger` marks an action that
  // removes something, such as Delete agent. Every other button stays quiet.
  let { children, icon, iconOnly = false, variant = 'default', ...props } = $props()
  const shown = $derived(variant === 'default' && props.type === 'submit' ? 'primary' : variant)
</script>
<button data-ui-button type="button" {...props} class:icon-only={iconOnly} data-variant={shown}>
  {#if icon}<LucideIcon name={icon} size={16} variant="action" />{/if}
  {@render children?.()}
</button>
<style>
  button { display: inline-flex; align-items: center; justify-content: center; flex: none; gap: 8px; min-height: 32px; min-width: 32px; box-sizing: border-box; padding: 6px 10px; font: var(--text-13) var(--font-human); line-height: 1.4; color: var(--ink); background: var(--faint); border: 1px solid transparent; border-radius: var(--radius-control); cursor: pointer; }
  button[data-variant="outline"], button[data-variant="capability"] { border-color: var(--border); background: var(--surface); }
  button:hover:not(:disabled), button[aria-expanded="true"] { border-color: var(--muted); }
  button[aria-pressed="true"] { background: var(--selected); color: var(--on-selected); border-color: var(--selected-edge); }
  button:focus-visible { background: var(--faint); color: var(--ink); }
  button[data-variant="capability"][aria-pressed="true"] { color: var(--signal); background: var(--signal-soft); border-color: transparent; }
  button[data-variant="primary"] { color: var(--on-accent); background: var(--accent); border-color: var(--accent); font-weight: var(--weight-semibold); }
  button[data-variant="primary"]:is(:hover:not(:disabled), :focus-visible) { color: var(--on-accent); background: color-mix(in srgb, var(--accent) 86%, var(--ink)); border-color: color-mix(in srgb, var(--accent) 86%, var(--ink)); }
  button[data-variant="danger"] { color: var(--danger); background: var(--danger-soft); border-color: var(--danger-edge); }
  button[data-variant="danger"]:is(:hover:not(:disabled), :focus-visible) { color: var(--danger); background: color-mix(in srgb, var(--danger) 20%, var(--surface)); border-color: var(--danger); }
  button:disabled { opacity: .5; cursor: default; }
  .icon-only { padding: 6px; }
</style>

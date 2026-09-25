<script>
  import LucideIcon from '../LucideIcon.svelte'
  let { children, ...props } = $props()
</script>
<summary {...props}><span class="chevron"><LucideIcon name="chevron-right" size={14} variant="action" /></span>{@render children?.()}</summary>
<style>
  summary { display: flex; align-items: center; gap: 8px; min-height: 24px; padding: 2px 0; list-style: none; cursor: pointer; color: var(--muted); font: var(--text-13) var(--font-human); }
  summary::-webkit-details-marker { display: none; }
  summary:focus-visible { background: var(--faint); color: var(--ink); }
  .chevron { display: inline-flex; flex: none; }
  :global(details[open]) > summary .chevron { transform: rotate(90deg); }
</style>

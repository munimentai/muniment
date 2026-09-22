<script>
  import LucideIcon from './LucideIcon.svelte'
  let { label = 'Close', disabled = false, onclick } = $props()
</script>

<button type="button" class="popup-close" aria-label={label} title={label} {disabled} {onclick}><LucideIcon name="x" variant="action" size={16} /></button>

<style>
  .popup-close { display: inline-flex; align-items: center; justify-content: center; flex: none; width: 28px; height: 28px; min-height: 28px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .popup-close:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .popup-close:disabled { opacity: .5; cursor: default; }
</style>

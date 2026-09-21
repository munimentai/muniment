<script>
  let { checked = $bindable(false), disabled = false, label, 'aria-label': ariaLabel, onchange } = $props()
</script>
<button type="button" role="switch" aria-label={label || ariaLabel} aria-checked={checked} {disabled} onclick={() => { checked = !checked; onchange?.(checked) }}><span></span></button>
<style>
  button { position: relative; flex: none; width: 34px; height: 24px; padding: 0; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); cursor: pointer; }
  span { position: absolute; top: 4px; left: 4px; width: 14px; height: 14px; border-radius: var(--radius-chip); background: var(--muted); transition: transform 120ms ease; }
  button[aria-checked="true"] { border-color: var(--signal); background: var(--signal-soft); }
  button[aria-checked="true"] span { transform: translateX(10px); background: var(--signal); }
  button:hover:not(:disabled) { background: var(--faint); }
  button:focus-visible { border-color: var(--ink); background: var(--faint); }
  button:disabled { opacity: .45; cursor: default; }
  @media (prefers-reduced-motion: reduce) { span { transition: none; } }
</style>

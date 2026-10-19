<script>
  let { checked = $bindable(false), disabled = false, label, 'aria-label': ariaLabel, onchange } = $props()
</script>
<button type="button" role="switch" aria-label={label || ariaLabel} aria-checked={checked} {disabled} onclick={() => { checked = !checked; onchange?.(checked) }}><span></span></button>
<style>
  /* A pill track and a round knob. Off is the hover fill with the muted knob at
     the left. On fills the track with the scheme accent and moves the knob,
     in the accent's own text color, to the right. */
  button { position: relative; flex: none; width: 40px; height: 24px; padding: 0; border: 1px solid var(--border); border-radius: var(--radius-pill); background: var(--faint); cursor: pointer; transition: background 120ms ease, border-color 120ms ease; }
  span { position: absolute; top: 2px; left: 2px; width: 18px; height: 18px; border-radius: 50%; background: var(--muted); transition: transform 120ms ease, background 120ms ease; }
  button[aria-checked="true"] { border-color: var(--accent); background: var(--accent); }
  button[aria-checked="true"] span { transform: translateX(16px); background: var(--on-accent); }
  button:hover:not(:disabled) { border-color: var(--muted); }
  button[aria-checked="true"]:hover:not(:disabled) { border-color: color-mix(in srgb, var(--accent) 86%, var(--ink)); background: color-mix(in srgb, var(--accent) 86%, var(--ink)); }
  button:focus-visible { border-color: var(--focus-ring); }
  button:disabled { opacity: .45; cursor: default; }
  :global(:root[data-motion="reduce"]) button, :global(:root[data-motion="reduce"]) span { transition: none; }
</style>

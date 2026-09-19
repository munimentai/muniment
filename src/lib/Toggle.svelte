<script>
  let { checked = $bindable(false), disabled = false, onchange, 'aria-label': label } = $props()
</script>

<span class="toggle">
  <input type="checkbox" bind:checked {disabled} {onchange} aria-label={label} />
  <span class="track" aria-hidden="true"><span class="knob"></span></span>
</span>

<style>
  .toggle { position: relative; display: inline-block; flex: none; width: 30px; height: 18px; vertical-align: middle; }
  input { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; cursor: pointer; }
  .track { display: block; width: 100%; height: 100%; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--paper); pointer-events: none; }
  .knob { position: absolute; top: 3px; left: 3px; width: 12px; height: 12px; border-radius: var(--radius-chip); background: var(--muted); transition: transform 120ms ease, background 120ms ease; }
  input:checked + .track { border-color: var(--signal); background: var(--signal-soft); }
  input:checked + .track .knob { transform: translateX(12px); background: var(--signal); }
  input:hover:not(:disabled) + .track, input:focus-visible + .track { background: var(--faint); }
  input:disabled { cursor: default; }
  input:disabled + .track { opacity: 0.5; }
  @media (prefers-reduced-motion: reduce) { .knob { transition: none; } }
</style>

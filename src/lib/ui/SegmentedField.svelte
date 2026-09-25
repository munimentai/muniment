<script>
  let { label, value, options, onchange } = $props()
  function keys(event) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const index = options.findIndex(option => option.value === value)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : options.length - 1)) % options.length
    onchange(options[next].value)
    event.currentTarget.querySelectorAll('button')[next]?.focus()
  }
</script>
<div class="field">
  <span>{label}</span>
  <div class="segments" role="radiogroup" aria-label={label} onkeydown={keys}>
    {#each options as option (option.value)}
      <button data-ui-choice type="button" role="radio" aria-checked={value === option.value} tabindex={value === option.value ? 0 : -1} onclick={() => onchange(option.value)}>{option.label}</button>
    {/each}
  </div>
</div>
<style>
  .field { display: grid; gap: 6px; min-width: 0; color: var(--muted); font: var(--text-13) var(--font-human); }
  .segments { display: flex; border: 1px solid var(--border); border-radius: var(--radius-control); overflow: hidden; }
  button { flex: 1; min-height: 32px; padding: 7px 10px; border: 0; background: var(--surface); color: var(--muted); font: inherit; cursor: pointer; white-space: nowrap; }
  button + button { border-left: 1px solid var(--border); }
  button[aria-checked=true] { background: var(--faint); color: var(--ink); }
  button:focus-visible { background: var(--faint); color: var(--ink); }
</style>

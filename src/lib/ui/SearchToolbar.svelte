<script>
  import Button from './Button.svelte'
  let { value = $bindable(''), label, placeholder, filters = false, expanded = false, controls, count = 0, ontoggle, oninput, actions } = $props()
</script>
<div class="toolbar">
  {#if filters}<Button icon="sliders-vertical" iconOnly aria-label={count ? `Filters (${count} active)` : 'Filters'} aria-expanded={expanded} aria-controls={controls} onclick={ontoggle} />{/if}
  <input type="search" aria-label={label} {placeholder} bind:value {oninput}>
  {@render actions?.()}
</div>
<style>
  .toolbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; min-width: 0; }
  input { flex: 1; min-width: 150px; box-sizing: border-box; min-height: 32px; padding: 7px 10px; font: var(--text-13) var(--font-human); line-height: 1.4; background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); }
  input:focus-visible { background: var(--faint); color: var(--ink); }
</style>

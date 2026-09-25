<script>
  import { tick, onMount } from 'svelte'
  import ProviderLogo from '../ProviderLogo.svelte'
  import LucideIcon from '../LucideIcon.svelte'
  let { label, value, options, onchange, disabled = false, placeholder = 'Choose', inline = true } = $props()
  let open = $state(false), root = $state(), trigger = $state()
  const selected = $derived(options.find(option => option.value === value))
  async function show() { open = !open; if (open) { await tick(); root.querySelector('[aria-checked="true"], [role="menuitemradio"]')?.focus() } }
  function close() { open = false; trigger?.focus() }
  onMount(() => {
    window.addEventListener('keydown', keys, true)
    return () => window.removeEventListener('keydown', keys, true)
  })
  function keys(event) {
    if (!open) return
    if (event.key === 'Escape') { event.preventDefault(); event.stopImmediatePropagation(); close() }
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault()
      const items = [...root.querySelectorAll('[role="menuitemradio"]')]
      const index = items.indexOf(document.activeElement)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length
      items[next]?.focus()
    }
  }
</script>
<svelte:window onpointerdown={event => { if (open && !root?.contains(event.target)) open = false }} />
<div class="field" class:inline bind:this={root}>
  <span>{label}</span>
  <div class="control">
    <button class="trigger" type="button" bind:this={trigger} {disabled} aria-label={`${label}: ${selected?.label ?? placeholder}`} aria-haspopup="menu" aria-expanded={open} onclick={show}>
      {#if selected?.provider}<ProviderLogo provider={selected.provider} size={16} />{/if}<span>{selected?.label ?? placeholder}</span><LucideIcon name="chevron-down" size={14} variant="action" />
    </button>
    {#if open}<div class="choices" role="menu" aria-label={label}>
      {#each options as option (option.value)}<button type="button" role="menuitemradio" aria-checked={option.value === value} onclick={() => { close(); onchange(option.value) }}>
        {#if option.provider}<ProviderLogo provider={option.provider} size={16} />{/if}<span>{option.label}</span>{#if option.value === value}<LucideIcon name="check" size={14} variant="action" />{/if}
      </button>{/each}
    </div>{/if}
  </div>
</div>
<style>
  .field { display: grid; gap: 6px; font: var(--text-13) var(--font-human); min-width: 0; }
  .inline { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .control { position: relative; min-width: 0; } .inline .control { width: min(360px, 100%); }
  button { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 32px; padding: 7px 10px; box-sizing: border-box; font: inherit; line-height: 1.4; text-align: left; color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); cursor: pointer; }
  button span { flex: 1; } button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .choices { position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 3; display: grid; gap: 2px; max-height: 240px; overflow: auto; padding: 4px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); box-shadow: var(--shadow-overlay); }
  .choices button { border-color: transparent; } .choices button:hover, .choices button[aria-checked="true"] { background: var(--faint); }
</style>

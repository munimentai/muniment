<script>
  // The sidebar's runtime switcher: one row that names the runtime the app
  // shows, and a menu of every runtime with its state and Manage runtimes.
  // It shows only once a runtime is paired, so a single runtime leaves no row.
  import { tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { activeRuntime, kindIcon, pairedRuntimes, statusLabel, statusTone } from './runtimes.js'

  let { list = null, disabled = false, onselect = () => {}, onmanage = () => {} } = $props()
  let open = $state(false)
  let root = $state()
  let toggle = $state()
  const active = $derived(activeRuntime(list))
  const quiet = $derived(!active || active.local || active.status === 'online')

  $effect(() => {
    if (!open) return
    const outside = (event) => { if (!root?.contains(event.target)) open = false }
    window.addEventListener('pointerdown', outside, true)
    return () => window.removeEventListener('pointerdown', outside, true)
  })

  async function show() {
    open = !open
    if (!open) return
    await tick()
    root?.querySelector('[role="menu"] button:not(:disabled)')?.focus()
  }

  function keydown(event) {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      open = false
      toggle?.focus()
      return
    }
    if (!['ArrowDown', 'ArrowUp'].includes(event.key)) return
    event.preventDefault()
    const items = [...event.currentTarget.querySelectorAll('button:not(:disabled)')]
    const index = items.indexOf(document.activeElement)
    items[(index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus()
  }

  function pick(runtime) {
    open = false
    if (!runtime.active) onselect(runtime)
  }
</script>

{#if active && pairedRuntimes(list).length}
  <div class="runtime-switcher" bind:this={root}>
    <button bind:this={toggle} type="button" class="switcher" aria-haspopup="menu" aria-expanded={open} aria-label="Runtime: {active.name}. Switch runtime" onclick={show}>
      <LucideIcon name={kindIcon(active)} />
      <span class="name">{active.name}</span>
      {#if !quiet}<em class="state" data-tone={statusTone(active)}>{statusLabel(active)}</em>{/if}
      <LucideIcon name="chevrons-up-down" size={14} />
    </button>
    {#if open}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="runtime-menu" role="menu" tabindex="-1" aria-label="Runtimes" onkeydown={keydown}>
        {#each list.runtimes as runtime (runtime.id)}
          <button type="button" role="menuitemradio" aria-checked={runtime.active} disabled={disabled && !runtime.active} onclick={() => pick(runtime)}>
            <LucideIcon name={kindIcon(runtime)} />
            <span class="name">{runtime.name}</span>
            <span class="state" data-tone={statusTone(runtime)}>{runtime.active ? 'In use' : statusLabel(runtime)}</span>
          </button>
        {/each}
        <div class="separator" role="separator"></div>
        <button type="button" role="menuitem" class="manage" onclick={() => { open = false; onmanage() }}>
          <LucideIcon name="settings" /><span class="name">Manage runtimes</span>
        </button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .runtime-switcher { position: relative; margin-top: 2px; }
  .switcher, .runtime-menu button { width: 100%; display: flex; align-items: center; gap: 8px; min-height: 28px; box-sizing: border-box; padding: 4px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .switcher:hover, .switcher[aria-expanded="true"], .runtime-menu button:hover:not(:disabled), .runtime-menu button:focus-visible { background: var(--faint); }
  .switcher :global(.lucide), .runtime-menu :global(.lucide) { flex: 0 0 18px; width: 18px; height: 18px; color: var(--muted); }
  .switcher :global(.lucide:last-child) { flex-basis: 14px; width: 14px; height: 14px; }
  .name { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .state { flex: none; color: var(--muted); font: normal var(--text-12) var(--font-human); }
  .state[data-tone="caution"] { color: var(--ochre); }
  .state[data-tone="critical"] { color: var(--oxide); }
  .runtime-menu { position: absolute; z-index: 4; top: calc(100% + 4px); left: 0; right: 0; display: grid; gap: 2px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .runtime-menu button[aria-checked="true"] .name { font-weight: var(--weight-semibold, 600); }
  .runtime-menu button:disabled { opacity: .5; cursor: default; }
  .separator { height: 1px; margin: 2px 4px; background: var(--border); }
  .runtime-menu .manage { color: var(--muted); }
</style>

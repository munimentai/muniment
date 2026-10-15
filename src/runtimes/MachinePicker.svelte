<script>
  // The composer's machine chip: the machine a new thread starts on, with a
  // menu of every machine and its state, or the machine an open thread lives
  // on, which it keeps. It shows only once a machine is paired.
  import { tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { activeRuntime, kindIcon, pairedRuntimes, statusLabel, statusTone } from './runtimes.js'

  let { list = null, locked = null, disabled = false, onselect = () => {}, onmanage = () => {} } = $props()
  let open = $state(false)
  let root = $state()
  let toggle = $state()
  let menu = $state()
  const active = $derived(activeRuntime(list))
  const held = $derived(locked ? list?.runtimes?.find((runtime) => runtime.id === locked) ?? null : null)
  const shown = $derived(held ?? active)

  $effect(() => {
    if (!open) return
    const outside = (event) => { if (!root?.contains(event.target) && !menu?.contains(event.target)) open = false }
    window.addEventListener('pointerdown', outside, true)
    return () => window.removeEventListener('pointerdown', outside, true)
  })

  // The menu opens above the chip, in viewport coordinates.
  let place = $state('')

  async function show() {
    open = !open
    if (!open) return
    const box = toggle.getBoundingClientRect()
    place = `left: ${Math.round(box.left)}px; bottom: ${Math.round(window.innerHeight - box.top + 6)}px;`
    await tick()
    menu?.querySelector('button:not(:disabled)')?.focus()
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

  // The composer clips what overflows it, so the open menu lives in the body.
  function portal(node) {
    document.body.appendChild(node)
    return { destroy: () => node.remove() }
  }

  // A paired machine takes a new thread only while it answers.
  const reachable = (runtime) => runtime.local || runtime.status === 'online'

  function pick(runtime) {
    open = false
    if (!runtime.active) onselect(runtime)
  }
</script>

{#if shown && pairedRuntimes(list).length}
  {#if locked}
    <span class="machine-chip locked" role="note" aria-label="This thread stays on {shown.name}"><LucideIcon name={kindIcon(shown)} size={14} /><span class="name">{shown.name}</span></span>
  {:else}
    <span class="machine-picker" bind:this={root}>
      <button bind:this={toggle} type="button" class="machine-chip" aria-haspopup="menu" aria-expanded={open} aria-label="Machine: {shown.name}. Choose a machine" {disabled} onclick={show}>
        <LucideIcon name={kindIcon(shown)} size={14} />
        <span class="name">{shown.name}</span>
        <LucideIcon name="chevrons-up-down" size={12} variant="action" />
      </button>
      {#if open}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div class="machine-menu" role="menu" tabindex="-1" aria-label="Machines" style={place} bind:this={menu} use:portal onkeydown={keydown}>
          {#each list.runtimes as runtime (runtime.id)}
            <button type="button" role="menuitemradio" aria-checked={runtime.active} disabled={!reachable(runtime)} onclick={() => pick(runtime)}>
              <LucideIcon name={kindIcon(runtime)} size={14} />
              <span class="name">{runtime.name}</span>
              <span class="state" data-tone={statusTone(runtime)}>{statusLabel(runtime)}</span>
            </button>
          {/each}
          <div class="separator" role="separator"></div>
          <button type="button" role="menuitem" class="manage" onclick={() => { open = false; onmanage() }}>
            <LucideIcon name="plus" size={14} /><span class="name">Add a machine</span>
          </button>
        </div>
      {/if}
    </span>
  {/if}
{/if}

<style>
  .machine-picker { position: relative; flex: none; display: inline-flex; }
  .machine-chip { flex: none; display: inline-flex; align-items: center; gap: 5px; min-height: 24px; max-width: 180px; padding: 3px 4px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  button.machine-chip { cursor: pointer; }
  button.machine-chip:hover:not(:disabled), button.machine-chip[aria-expanded="true"] { background: var(--faint); }
  .machine-chip.locked { color: var(--muted); }
  .machine-chip :global(.lucide) { flex: none; color: var(--muted); }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; line-height: 16px; }
  .machine-menu { position: fixed; z-index: 30; display: grid; gap: 2px; min-width: 240px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .machine-menu button { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 28px; padding: 4px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .machine-menu button:hover:not(:disabled), .machine-menu button:focus-visible { background: var(--faint); }
  .machine-menu button[aria-checked="true"] .name { font-weight: var(--weight-semibold, 600); }
  .machine-menu button:disabled { color: var(--muted); cursor: default; }
  .machine-menu :global(.lucide) { flex: none; color: var(--muted); }
  .machine-menu .name { flex: 1; }
  .state { flex: none; margin-left: auto; color: var(--muted); font-size: var(--text-12); }
  .state[data-tone="caution"] { color: var(--ochre); }
  .state[data-tone="critical"] { color: var(--oxide); }
  .separator { height: 1px; margin: 2px 4px; background: var(--border); }
  .manage { color: var(--muted) !important; }
</style>

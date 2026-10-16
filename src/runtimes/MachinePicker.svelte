<script>
  // The composer's machine chip: the machine a new thread starts on, with a
  // menu of every machine and whether it answers, or the machine an open
  // thread lives on, which it keeps. It shows only once a machine is paired.
  // The menu is a composer panel, so opening it closes any other.
  import { onMount, tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ComposerMenuButton from '../composer/ComposerMenuButton.svelte'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from '../lib/composer-panels.js'
  import { activeRuntime, kindIcon, pairedRuntimes, statusLabel } from './runtimes.js'

  let { list = null, locked = null, disabled = false, onselect = () => {}, onmanage = () => {} } = $props()
  let open = $state(false)
  let root = $state()
  let toggle = $state()
  let menu = $state()
  const active = $derived(activeRuntime(list))
  const held = $derived(locked ? list?.runtimes?.find((runtime) => runtime.id === locked) ?? null : null)
  const shown = $derived(held ?? active)

  onMount(() => {
    const follow = async (event) => {
      open = event.detail === 'machine'
      if (!open) return
      await tick()
      menu?.querySelector('button')?.focus()
    }
    window.addEventListener(COMPOSER_PANEL_EVENT, follow)
    return () => window.removeEventListener(COMPOSER_PANEL_EVENT, follow)
  })

  $effect(() => {
    if (!open) return
    const outside = (event) => { if (!root?.contains(event.target)) openComposerPanel(null) }
    window.addEventListener('pointerdown', outside, true)
    return () => window.removeEventListener('pointerdown', outside, true)
  })

  function close() {
    openComposerPanel(null)
    open = false
  }

  function keydown(event) {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      close()
      toggle?.focus()
      return
    }
    if (!['ArrowDown', 'ArrowUp'].includes(event.key)) return
    event.preventDefault()
    const items = [...event.currentTarget.querySelectorAll('button:not(:disabled)')]
    const index = items.indexOf(document.activeElement)
    items[(index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus()
  }

  // A paired machine takes a new thread only while it answers. One that does
  // not opens Settings → Machines, which names the next step.
  const reachable = (runtime) => runtime.local || runtime.status === 'online'

  function pick(runtime) {
    close()
    if (!reachable(runtime)) onmanage()
    else if (!runtime.active) onselect(runtime)
  }
</script>

{#if shown && pairedRuntimes(list).length}
  {#if locked}
    <span class="machine-chip" role="note" aria-label="This thread stays on {shown.name}"><LucideIcon name={kindIcon(shown)} size={14} /><span class="name">{shown.name}</span></span>
  {:else}
    <span class="machine-picker" bind:this={root}>
      <ComposerMenuButton menu="machine" {open} chevrons label="Machine: {shown.name}. Choose a machine" text={shown.name} {disabled} bind:element={toggle} onclick={() => openComposerPanel(open ? null : 'machine')}>
        {#snippet icon()}<LucideIcon name={kindIcon(shown)} size={14} />{/snippet}
        {#snippet compact()}<LucideIcon name="network" size={14} />{/snippet}
      </ComposerMenuButton>
      {#if open}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div data-composer-panel data-panel="machine-menu" data-panel-variant="overlay" class="machine-menu" role="menu" tabindex="-1" aria-label="Machines" bind:this={menu} onkeydown={keydown}>
          {#each list.runtimes as runtime (runtime.id)}
            {@const online = reachable(runtime)}
            <button type="button" role="menuitemradio" aria-checked={runtime.active} onclick={() => pick(runtime)}>
              <LucideIcon name={kindIcon(runtime)} size={14} />
              <span class="name">{runtime.name}</span>
              <span class="state" class:online role="img" aria-label={online ? 'Online' : statusLabel(runtime)}><LucideIcon name={online ? 'globe' : 'globe-off'} variant="action" size={14} /></span>
            </button>
          {/each}
          <div class="separator" role="separator"></div>
          <button type="button" role="menuitem" class="manage" onclick={() => { close(); onmanage() }}>
            <LucideIcon name="plus" size={14} /><span class="name">Add a machine</span>
          </button>
        </div>
      {/if}
    </span>
  {/if}
{/if}

<style>
  .machine-picker { flex: none; display: inline-flex; }
  .machine-chip { flex: none; display: inline-flex; align-items: center; gap: 5px; min-height: 24px; max-width: 180px; padding: 3px 4px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  .machine-chip :global(.lucide) { flex: none; color: var(--muted); }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; line-height: 16px; }
  .machine-menu { display: grid; gap: 2px; width: min(280px, 100%); padding: 4px; }
  .machine-menu button { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 28px; padding: 4px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .machine-menu button:hover, .machine-menu button:focus-visible { background: var(--faint); }
  .machine-menu button[aria-checked="true"] .name { font-weight: var(--weight-semibold, 600); }
  .machine-menu :global(.lucide) { flex: none; color: var(--muted); }
  .machine-menu .name { flex: 1; }
  /* A green globe marks a machine that answers, and a red one with a slash a machine that does not. */
  .state { flex: none; display: inline-flex; margin-left: auto; color: var(--oxide); }
  .state.online { color: var(--signal); }
  .machine-menu .state :global(.lucide) { color: inherit; }
  .separator { height: 1px; margin: 2px 4px; background: var(--border); }
  .manage { color: var(--muted) !important; }
</style>

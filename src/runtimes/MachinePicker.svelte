<script>
  // The composer's machine chip: the machine a new thread starts on, with a
  // menu of every machine and whether it answers, or the machine an open
  // thread lives on, which it keeps. Under the paired machines, the menu
  // lists the fleet members this app does not reach itself, containers
  // included: a new thread can start on any of them, and core forwards it
  // there. An open thread another member owns names that owner. It shows
  // only once a machine is paired or a fleet has other members. The menu is
  // a composer panel, so opening it closes any other.
  import { onMount, tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ComposerMenuButton from '../composer/ComposerMenuButton.svelte'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from '../lib/composer-panels.js'
  import { activeRuntime, kindIcon, machineName, pairedRuntimes, statusLabel } from './runtimes.js'

  // `source` names where the new thread's machine comes from, such as
  // `from project Homelab` or `default`.
  let { list = null, locked = null, owner = null, fleet = [], offload = null, source = '', disabled = false, onselect = () => {}, onoffload = () => {}, onmanage = () => {} } = $props()
  let open = $state(false)
  let root = $state()
  let toggle = $state()
  let menu = $state()
  const active = $derived(activeRuntime(list))
  const held = $derived(locked ? list?.runtimes?.find((runtime) => runtime.id === locked) ?? null : null)
  const shown = $derived(owner ?? held ?? (offload && !locked ? offload : active))
  const shownName = $derived(machineName(shown))
  const visible = $derived(!!shown && (pairedRuntimes(list).length > 0 || fleet.length > 0 || !!owner))

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
    else if (!runtime.active || offload) onselect(runtime)
  }

  // A fleet member takes a new thread even while it is away: core queues
  // the thread until it answers.
  function pickMember(member) {
    close()
    onoffload(member)
  }
</script>

{#if visible}
  {#if locked || owner}
    <span class="machine-chip" role="note" aria-label="This thread stays on {shownName}"><LucideIcon name={kindIcon(shown)} size={14} /><span class="name">{owner ? `on ${shownName}` : shownName}</span>{#if owner?.online === false}<span class="state" role="img" aria-label="Offline"><LucideIcon name="globe-off" variant="action" size={14} /></span>{/if}</span>
  {:else}
    <span class="machine-picker" bind:this={root}>
      <ComposerMenuButton menu="machine" {open} chevrons label="Machine: {shownName}. Choose a machine" text={shownName} {disabled} bind:element={toggle} onclick={() => openComposerPanel(open ? null : 'machine')}>
        {#snippet icon()}<LucideIcon name={kindIcon(shown)} size={14} />{/snippet}
        {#snippet compact()}<LucideIcon name="network" size={14} />{/snippet}
      </ComposerMenuButton>
      {#if open}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div data-composer-panel data-panel="machine-menu" data-panel-variant="overlay" class="machine-menu" role="menu" tabindex="-1" aria-label="Machines" bind:this={menu} onkeydown={keydown}>
          {#each list?.runtimes ?? [] as runtime (runtime.id)}
            {@const online = reachable(runtime)}
            <button type="button" role="menuitemradio" aria-checked={runtime.active && !offload} onclick={() => pick(runtime)}>
              <LucideIcon name={kindIcon(runtime)} size={14} />
              <span class="name">{machineName(runtime)}</span>
              <span class="state" class:online role="img" aria-label={online ? 'Online' : statusLabel(runtime)}><LucideIcon name={online ? 'globe' : 'globe-off'} variant="action" size={14} /></span>
            </button>
          {/each}
          {#if fleet.length}
            <div class="separator" role="separator"></div>
            <p class="heading" aria-hidden="true">Fleet</p>
            {#each fleet as member (member.coreId)}
              <button type="button" role="menuitemradio" aria-checked={offload?.coreId === member.coreId} aria-label="{member.name}{member.online ? '' : ', offline'}" onclick={() => pickMember(member)}>
                <LucideIcon name={kindIcon(member)} size={14} />
                <span class="name">{member.name}</span>
                <span class="state" class:online={member.online} role="img" aria-label={member.online ? 'Online' : 'Offline'}><LucideIcon name={member.online ? 'globe' : 'globe-off'} variant="action" size={14} /></span>
              </button>
            {/each}
          {/if}
          {#if source}<p class="source">{shownName} · {source}</p>{/if}
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
  .machine-chip { flex: none; display: inline-flex; align-items: center; gap: 5px; min-height: 24px; max-width: 180px; padding: 3px 4px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-12) var(--font-human); white-space: nowrap; }
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
  .source { margin: 2px 0 0; padding: 4px 8px 2px; border-top: 1px solid var(--border); color: var(--muted); font: var(--text-12) var(--font-human); }
  .heading { margin: 2px 8px 0; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .machine-chip .state { margin-left: 0; color: var(--ochre); }
  .machine-chip .state :global(.lucide) { color: inherit; }
</style>

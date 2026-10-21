<script>
  // The composer's machine chip: the machine a new thread starts on, or the
  // machine an open thread lives on, which it keeps. It always opens a menu
  // of every paired machine with whether it answers: one that does not is a
  // disabled choice tagged with its state, such as Offline, as Settings →
  // Machines words it. Under the paired machines, the menu lists the fleet
  // members this app does not reach itself, containers included: a new
  // thread can start on any that answers, and core forwards it there. An
  // open thread another member owns names that owner. An open thread's menu
  // checks its machine and offers no other choice. A narrow composer keeps
  // the chip to the picked machine's icon, as the model and mode chips keep
  // theirs, with the machine in its name and hover label. The chip shows only once
  // a machine is paired or a fleet has other members. The menu is a composer
  // panel, so opening it closes any other.
  import { onMount, tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ComposerMenuButton from '../composer/ComposerMenuButton.svelte'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from '../lib/composer-panels.js'
  import { activeRuntime, kindIcon, machineName, pairedRuntimes, statusLabel } from './runtimes.js'
  import { followPointer } from '../lib/menu-pointer.js'
  import StatusChip from '../lib/ui/StatusChip.svelte'
  import { machineStatusTone } from '../lib/status-tone.js'

  // `source` names where the new thread's machine comes from, such as
  // `from project Homelab` or `default`. `disabled` keeps the menu open to
  // read while no choice can change, as during a reply.
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
  // An open thread keeps its machine, so its menu only shows the machines.
  const kept = $derived(!!locked || !!owner)
  const fixed = $derived(kept || disabled)

  onMount(() => {
    const follow = async (event) => {
      open = event.detail === 'machine'
      if (!open) return
      await tick()
      menu?.querySelector('button:not(:disabled)')?.focus()
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

  // A machine takes a new thread only while it answers.
  const reachable = (runtime) => runtime.local || runtime.status === 'online'
  const checked = (runtime) => kept ? !owner && runtime.id === locked : runtime.active && !offload

  function pick(runtime) {
    if (fixed || !reachable(runtime)) return
    close()
    if (!runtime.active || offload) onselect(runtime)
  }

  function pickMember(member) {
    if (fixed || !member.online) return
    close()
    onoffload(member)
  }
</script>

{#if visible}
  <span class="machine-picker" bind:this={root}>
    <ComposerMenuButton menu="machine" {open} chevrons label={kept ? `Machine: ${owner ? `on ${shownName}` : shownName}${owner?.online === false ? ', offline' : ''}. This thread stays on it` : `Machine: ${shownName}. Choose a machine`} text={owner ? `on ${shownName}` : shownName} bind:element={toggle} onclick={() => openComposerPanel(open ? null : 'machine')}>
      {#snippet icon()}<LucideIcon name={kindIcon(shown)} size={14} />{#if owner?.online === false}<span class="away" aria-hidden="true"><LucideIcon name="globe-off" variant="action" size={14} /></span>{/if}{/snippet}
    </ComposerMenuButton>
    {#if open}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div data-composer-panel data-panel="machine-menu" data-panel-variant="overlay" class="machine-menu" role="menu" tabindex="-1" aria-label="Machines" bind:this={menu} use:followPointer onkeydown={keydown}>
        {#each list?.runtimes ?? [] as runtime (runtime.id)}
          {@const online = reachable(runtime)}
          <button type="button" role="menuitemradio" aria-checked={checked(runtime)} aria-label="{machineName(runtime)}{online ? '' : `, ${statusLabel(runtime)}`}" disabled={!online || fixed} onclick={() => pick(runtime)}>
            <LucideIcon name={kindIcon(runtime)} size={14} />
            <span class="name">{machineName(runtime)}</span>
            {#if online}<span class="state online" role="img" aria-label="Online"><LucideIcon name="globe" variant="action" size={14} /></span>{:else}<StatusChip tone={machineStatusTone(runtime.status)}>{statusLabel(runtime)}</StatusChip>{/if}
          </button>
        {/each}
        {#if fleet.length}
          <div class="separator" role="separator"></div>
          <p class="heading" aria-hidden="true">Fleet</p>
          {#each fleet as member (member.coreId)}
            <button type="button" role="menuitemradio" aria-checked={owner ? owner.id === member.coreId : offload?.coreId === member.coreId} aria-label="{member.name}{member.online ? '' : ', Offline'}" disabled={!member.online || fixed} onclick={() => pickMember(member)}>
              <LucideIcon name={kindIcon(member)} size={14} />
              <span class="name">{member.name}</span>
              {#if member.online}<span class="state online" role="img" aria-label="Online"><LucideIcon name="globe" variant="action" size={14} /></span>{:else}<StatusChip tone="danger">Offline</StatusChip>{/if}
            </button>
          {/each}
        {/if}
        {#if kept}<p class="source">This thread stays on {owner ? owner.name : shownName}.</p>{:else if source}<p class="source">{shownName} · {source}</p>{/if}
        <div class="separator" role="separator"></div>
        <button type="button" role="menuitem" class="manage" onclick={() => { close(); onmanage() }}>
          <LucideIcon name="plus" size={14} /><span class="name">Add a machine</span>
        </button>
      </div>
    {/if}
  </span>
{/if}

<style>
  .machine-picker { flex: none; display: inline-flex; }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; line-height: 16px; }
  .machine-menu { display: grid; gap: 2px; width: min(280px, 100%); padding: 4px; }
  .machine-menu button { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 28px; padding: 4px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .machine-menu button:hover:not(:disabled) { background: var(--faint); }
  /* A machine that does not answer stays listed, muted, with its state. */
  .machine-menu button:disabled { cursor: default; }
  .machine-menu button:disabled:not([aria-checked="true"]) .name { color: var(--muted); }
  .machine-menu button[aria-checked="true"] .name { font-weight: var(--weight-semibold, 600); }
  .machine-menu :global(.lucide) { flex: none; color: var(--muted); }
  .machine-menu .name { flex: 1; }
  /* A green globe marks a machine that answers. One that does not shows its state chip. */
  .state { flex: none; display: inline-flex; margin-left: auto; }
  .state.online { color: var(--success); }
  .machine-menu :global(.status-chip) { margin-left: auto; }
  .machine-menu .state :global(.lucide) { color: inherit; }
  .separator { height: 1px; margin: 2px 4px; background: var(--border); }
  .manage { color: var(--muted) !important; }
  .source { margin: 2px 0 0; padding: 4px 8px 2px; border-top: 1px solid var(--border); color: var(--muted); font: var(--text-12) var(--font-human); }
  .heading { margin: 2px 8px 0; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  /* The open thread's owner that is away keeps the warning slashed globe beside its shape. */
  .away { display: inline-flex; color: var(--warning); }
  .away :global(.lucide) { color: inherit; }
</style>

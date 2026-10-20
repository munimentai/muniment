<script>
  // A menu in the composer menus' style for the agent profile: one row per
  // choice with its icon or avatar, its label, an optional tag and a check on
  // the chosen one. `multiple` makes each row a checkbox and keeps the menu
  // open. A single choice or Escape closes it, and `note` adds a line at its foot.
  import { onMount, tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import { followPointer } from '../lib/menu-pointer.js'

  let { label, options = [], value = null, values = [], multiple = false, note = '', empty = '', onchoose, onclose } = $props()
  let menu = $state()

  const items = () => [...(menu?.querySelectorAll('[role^="menuitem"]:not(:disabled)') ?? [])]

  onMount(() => {
    void tick().then(() => (items().find((item) => item.getAttribute('aria-checked') === 'true') ?? items()[0])?.focus())
  })

  function windowKeydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onclose()
  }

  // Arrows, Home and End move between the choices.
  function menuKeydown(event) {
    const all = items()
    if (!all.length || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const index = all.indexOf(document.activeElement)
    const next = event.key === 'Home' ? 0
      : event.key === 'End' ? all.length - 1
        : (index + (event.key === 'ArrowDown' ? 1 : -1) + all.length) % all.length
    all[next].focus()
  }
</script>

<svelte:window onkeydown={windowKeydown} />

<div data-panel="choice-menu" data-panel-variant="overlay" class="choice-menu" role="menu" tabindex="-1" aria-label={label} bind:this={menu} use:followPointer onkeydown={menuKeydown}>
  {#each options as option (option.value)}
    {@const checked = multiple ? values.includes(option.value) : option.value === value}
    <button type="button" role={multiple ? 'menuitemcheckbox' : 'menuitemradio'} class="choice-row" aria-checked={checked} disabled={option.disabled} onclick={() => onchoose(option.value, !checked)}>
      {#if option.agent}<AgentAvatar agent={option.agent} size={16} />{:else if option.icon}<LucideIcon name={option.icon} variant="action" size={14} />{/if}
      <span class="choice-label">{option.label}</span>
      {#if option.tag}<span class="tag">{option.tag}</span>{/if}
      {#if checked}<LucideIcon name="check" variant="action" size={14} />{/if}
    </button>
  {:else}
    {#if empty}<p class="note">{empty}</p>{/if}
  {/each}
  {#if note}<p class="note">{note}</p>{/if}
</div>

<style>
  .choice-menu { width: min(280px, 100%); display: flex; flex-direction: column; padding: 6px; overflow-y: auto; outline: 0; }
  .choice-row { display: flex; width: 100%; align-items: center; gap: 8px; min-height: 30px; padding: 5px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .choice-row:hover:not(:disabled), .choice-row[aria-checked="true"] { background: var(--faint); }
  .choice-row:disabled { color: var(--muted); cursor: default; }
  .choice-row > :global(svg) { flex: none; }
  .choice-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tag { flex: none; color: var(--muted); font-size: var(--text-12); }
  .note { margin: 4px 0 0; padding: 7px 8px 3px; border-top: 1px solid var(--border); color: var(--muted); font-size: var(--text-12); line-height: 1.4; }
  .note:first-child { margin: 0; padding-top: 3px; border-top: 0; }
</style>

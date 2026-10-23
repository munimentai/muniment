<script>
  // The composer's thinking level menu: the levels the chip's model offers,
  // the one a send carries checked. A choice or Escape closes it.
  import { onMount, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { thinkingLabel } from './thinking-level.js'
  import { followPointer } from './menu-pointer.js'

  let { levels, current, onchoose, onclose } = $props()
  let menu = $state()

  const items = () => [...(menu?.querySelectorAll('[role="menuitemradio"]') ?? [])]

  onMount(() => {
    void tick().then(() => (items().find((item) => item.getAttribute('aria-checked') === 'true') ?? items()[0])?.focus())
  })

  function windowKeydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onclose()
  }

  // Arrows, Home and End move between the levels.
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

<div data-panel="thinking-picker" data-panel-variant="overlay" data-composer-panel class="thinking-picker" role="menu" tabindex="-1" aria-label="Thinking level" bind:this={menu} use:followPointer onkeydown={menuKeydown}>
  {#each levels as level (level)}
    <button type="button" role="menuitemradio" class="thinking-row" aria-checked={level === current} data-level={level} onclick={() => onchoose(level)}>
      <span>{thinkingLabel(level)}</span>
      {#if level === current}<LucideIcon name="check" variant="action" size={14} />{/if}
    </button>
  {/each}
</div>

<style>
  .thinking-picker { width: min(200px, 100%); display: flex; flex-direction: column; padding: 6px; overflow-y: auto; outline: 0; }
  .thinking-row { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 5px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .thinking-row:hover, .thinking-row[aria-checked="true"] { background: var(--faint); }
</style>

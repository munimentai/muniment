<script>
  // The composer's permission mode menu: core's modes under a Modes heading,
  // each with its icon, title and one line of description, the thread's mode
  // checked. An unavailable mode shows why and cannot be chosen. A choice or
  // Escape closes it, and Shift+Tab in the composer moves to the next mode.
  import { onMount, tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { modeIcon } from '../agents/permission-modes.js'

  let { modes, current, onchoose, onclose } = $props()
  let menu = $state()

  const items = () => [...(menu?.querySelectorAll('[role="menuitemradio"]:not(:disabled)') ?? [])]

  onMount(() => {
    void tick().then(() => (items().find((item) => item.getAttribute('aria-checked') === 'true') ?? items()[0])?.focus())
  })

  function windowKeydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onclose()
  }

  // Arrows, Home and End move between the modes that can be chosen.
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

<div data-panel="mode-picker" data-panel-variant="overlay" data-composer-panel class="mode-picker" role="menu" tabindex="-1" aria-labelledby="mode-picker-heading" bind:this={menu} onkeydown={menuKeydown}>
  <h3 id="mode-picker-heading" class="heading">Modes</h3>
  {#each modes as mode (mode.id)}
    {@const line = mode.available === false && mode.reason ? mode.reason : mode.description}
    <button type="button" role="menuitemradio" class="mode-row" aria-checked={mode.id === current} aria-label={mode.title} aria-describedby={`mode-line-${mode.id}`} data-mode={mode.id} disabled={mode.available === false} onclick={() => onchoose(mode.id)}>
      <LucideIcon name={modeIcon(mode)} variant="action" size={16} />
      <span class="mode-text"><span class="mode-title">{mode.title}</span><span class="mode-line" id={`mode-line-${mode.id}`}>{line}</span></span>
      {#if mode.id === current}<LucideIcon name="check" variant="action" size={14} />{/if}
    </button>
  {/each}
  <p class="hint"><kbd>Shift+Tab</kbd>cycles modes</p>
</div>

<style>
  .mode-picker { width: min(360px, 100%); display: flex; flex-direction: column; padding: 6px; overflow-y: auto; outline: 0; }
  .heading { margin: 4px 8px 4px; color: var(--muted); font: var(--text-12) var(--font-mono); font-weight: 400; }
  .mode-row { display: flex; width: 100%; align-items: center; gap: 10px; min-height: 30px; padding: 6px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .mode-row:hover:not(:disabled), .mode-row[aria-checked="true"] { background: var(--faint); }
  .mode-row:disabled { color: var(--muted); cursor: default; }
  .mode-row > :global(svg) { flex: none; }
  .mode-row > :global(svg:last-child:not(:first-child)) { margin-left: auto; }
  .mode-text { display: grid; gap: 1px; flex: 1; min-width: 0; }
  .mode-title { font-size: var(--text-13); }
  .mode-line { overflow: hidden; color: var(--muted); font-size: var(--text-12); line-height: 1.4; text-overflow: ellipsis; white-space: nowrap; }
  .hint { display: flex; align-items: center; gap: 6px; margin: 4px 0 0; padding: 7px 8px 3px; border-top: 1px solid var(--border); color: var(--muted); font-size: var(--text-12); }
  kbd { padding: 1px 5px; border-radius: var(--radius-chip); background: var(--faint); color: var(--ink); font: var(--text-12) var(--font-mono); }
</style>

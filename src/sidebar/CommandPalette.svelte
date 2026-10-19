<script>
  // The command palette. One field searches threads, pages and actions, and
  // core's `palette::rank` orders what it finds: with no words, what needs
  // the user, then recent threads, then actions; a leading `>` keeps the
  // actions alone. Each row names its shortcut. Arrows move, Return runs
  // the row, and Escape or a click outside closes the palette.
  import { onMount, tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { dialogDismiss } from '../lib/dialog-dismiss.js'
  import { ariaShortcut, rankPalette, shortcutLabel } from './palette.js'

  // `input(query)` answers core's input for a query, and `onrun(item)` runs a row.
  let { tauri, input, onrun, onclose } = $props()
  let query = $state('')
  let groups = $state.raw([])
  let active = $state(0)
  let field = $state()
  let list = $state()
  let request = 0
  const items = $derived(groups.flatMap((group) => group.items))

  onMount(() => {
    const previous = document.activeElement
    void tick().then(() => { field?.focus(); field?.select() })
    return () => { if (previous?.isConnected) previous.focus?.() }
  })

  $effect(() => {
    const text = query
    const next = ++request
    Promise.resolve(rankPalette(tauri, input(text))).then((answer) => {
      if (next !== request) return
      groups = Array.isArray(answer) ? answer : []
      active = 0
    }).catch(() => { if (next === request) groups = [] })
  })

  const optionId = (index) => `palette-option-${index}`
  function move(index) {
    if (!items.length) return
    active = (index + items.length) % items.length
    void tick().then(() => list?.querySelector(`#${optionId(active)}`)?.scrollIntoView?.({ block: 'nearest' }))
  }
  function run(item) {
    if (!item) return
    onclose()
    onrun(item)
  }
  function keydown(event) {
    if (event.key === 'ArrowDown') { event.preventDefault(); move(active + 1) }
    else if (event.key === 'ArrowUp') { event.preventDefault(); move(active - 1) }
    else if (event.key === 'Home' && event.metaKey) { event.preventDefault(); move(0) }
    else if (event.key === 'End' && event.metaKey) { event.preventDefault(); move(items.length - 1) }
    else if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); run(items[active]) }
    else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onclose() }
    else if (event.key === 'Tab') event.preventDefault()
  }
</script>

<div class="palette-backdrop" use:dialogDismiss={{ onclose }}>
  <div data-panel="palette" data-panel-variant="overlay" class="palette" role="dialog" aria-modal="true" aria-label="Search">
    <div class="palette-field">
      <LucideIcon name="search" size={16} />
      <input bind:this={field} bind:value={query} type="text" data-focus-frame role="combobox" aria-label="Search threads, pages and actions" aria-expanded={items.length > 0} aria-controls="palette-results" aria-activedescendant={items.length ? optionId(active) : undefined} aria-autocomplete="list" autocomplete="off" spellcheck="false" placeholder="Search threads, pages and actions" onkeydown={keydown} />
    </div>
    <div class="palette-results" id="palette-results" role="listbox" aria-label="Results" bind:this={list}>
      {#each groups as group, groupIndex (group.title)}
        {@const offset = groups.slice(0, groupIndex).reduce((count, entry) => count + entry.items.length, 0)}
        <div class="palette-group" role="group" aria-labelledby={`palette-group-${groupIndex}`}>
          <h3 id={`palette-group-${groupIndex}`} role="presentation">{group.title}</h3>
          {#each group.items as item, index (`${item.kind}:${item.id}`)}
            {@const position = offset + index}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div class="palette-row" role="option" id={optionId(position)} tabindex="-1" aria-selected={position === active} aria-keyshortcuts={ariaShortcut(item.shortcut)} data-kind={item.kind} onpointermove={() => { active = position }} onclick={() => run(item)}>
              <LucideIcon name={item.icon || 'circle'} size={16} />
              <span class="palette-title">{item.title}</span>
              {#if item.detail}<span class="palette-detail">{item.detail}</span>{/if}
              {#if item.shortcut}<kbd>{shortcutLabel(item.shortcut)}</kbd>{/if}
            </div>
          {/each}
        </div>
      {:else}
        <p class="palette-empty">{query.trim() ? 'Nothing matches.' : 'Loading…'}</p>
      {/each}
    </div>
    <p class="palette-hint"><kbd>&gt;</kbd>Actions only<kbd>↵</kbd>Open<kbd>Esc</kbd>Close</p>
  </div>
</div>

<style>
  .palette-backdrop { position: fixed; inset: 0; z-index: 30; display: flex; justify-content: center; align-items: flex-start; padding: 12vh 16px 16px; background: var(--overlay-backdrop); }
  .palette { width: min(600px, 100%); max-height: min(560px, 76vh); display: flex; flex-direction: column; overflow: hidden; font-family: var(--font-human); }
  .palette-field { display: flex; align-items: center; gap: 10px; padding: 12px 14px; border-bottom: 1px solid var(--border); color: var(--muted); }
  .palette-field input { flex: 1; min-width: 0; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: var(--text-15) var(--font-human); }
  .palette-results { flex: 1; min-height: 0; padding: 6px; overflow-y: auto; }
  .palette-group + .palette-group { margin-top: 6px; }
  h3 { margin: 4px 8px; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .palette-row { display: flex; align-items: center; gap: 10px; min-height: 32px; padding: 4px 8px; border-radius: var(--radius-control); color: var(--ink); font-size: var(--text-13); cursor: pointer; }
  .palette-row :global(svg) { flex: none; color: var(--muted); }
  .palette-row[aria-selected="true"] { background: var(--selected); color: var(--on-selected); }
  .palette-title { flex: 0 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .palette-detail { flex: 1 1 auto; min-width: 0; overflow: hidden; color: var(--muted); font-size: var(--text-12); white-space: nowrap; text-overflow: ellipsis; }
  .palette-row kbd { flex: none; margin-left: auto; }
  kbd { padding: 1px 5px; border-radius: var(--radius-chip); background: var(--faint); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .palette-empty { margin: 10px 8px; color: var(--muted); font-size: var(--text-13); }
  .palette-hint { display: flex; align-items: center; gap: 6px; margin: 0; padding: 8px 14px; border-top: 1px solid var(--border); color: var(--muted); font-size: var(--text-12); }
  .palette-hint kbd:not(:first-child) { margin-left: 8px; }
</style>

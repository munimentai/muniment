<script>
  import PopupClose from './PopupClose.svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { status = 'active', sort = 'recent', onchange } = $props()
  let open = $state(false)
  let root
  $effect(() => {
    if (!open) return
    const outside = event => { if (!root.contains(event.target)) open = false }
    const escape = event => { if (event.key === 'Escape') open = false }
    document.addEventListener('pointerdown', outside)
    document.addEventListener('keydown', escape)
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('keydown', escape) }
  })
</script>
<div class="thread-filter" bind:this={root}>
  <h3>Threads</h3>
  <button aria-label="Filter threads" aria-haspopup="dialog" aria-expanded={open} onclick={() => open = !open}><LucideIcon name="sliders-vertical" /></button>
  {#if open}<div class="filter-popover" data-panel="thread-filter" data-panel-variant="overlay" role="dialog" aria-label="Thread filters">
    <header><strong>Threads</strong><PopupClose label="Close thread filters" onclick={() => open = false} /></header>
    <label>Status<select value={status} onchange={event => onchange(event.currentTarget.value, sort)}><option value="active">Active</option><option value="archived">Archived</option><option value="all">All</option></select></label>
    <label>Sort by<select value={sort} onchange={event => onchange(status, event.currentTarget.value)}><option value="recent">Last activity</option><option value="title">Name</option></select></label>
    <button onclick={() => onchange('active', 'recent')}>Reset to defaults</button>
  </div>{/if}
</div>
<style>
.thread-filter { position: relative; display: flex; align-items: center; justify-content: space-between; margin: 10px 8px 4px; }
h3 { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
button { border: 0; background: transparent; color: var(--muted); padding: 4px; border-radius: var(--radius-control); cursor: pointer; font: var(--text-12) var(--font-human); }
button:hover { background: var(--faint); color: var(--ink); }
.filter-popover { position: absolute; right: 0; top: 100%; width: 220px; padding: 12px; z-index: 10; }
header, label { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; font: var(--text-12) var(--font-human); }
select { min-width: 0; max-width: 130px; }
</style>

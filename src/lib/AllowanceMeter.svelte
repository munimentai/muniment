<script>
  let { remaining, limited = false } = $props()
  const percent = $derived(Math.max(0, Math.min(100, Number(remaining) || 0)))
</script>
<div class="window-bar" role="meter" aria-label="Remaining allowance" aria-valuemin="0" aria-valuemax="100" aria-valuenow={percent} data-level={limited || percent < 25 ? 'low' : percent <= 60 ? 'medium' : 'plenty'}><span style:width={`${percent}%`}></span></div>
<style>
.window-bar { height: 4px; border-radius: var(--radius-chip); background: var(--border); }
.window-bar span { display: block; height: 100%; border-radius: var(--radius-chip); }
.window-bar[data-level="plenty"] span { background: var(--signal); }
.window-bar[data-level="medium"] span { background: var(--ochre); }
.window-bar[data-level="low"] span { background: var(--oxide); }
</style>

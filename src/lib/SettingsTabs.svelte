<script>
  import McpIcon from '../extend/McpIcon.svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { tabs, value, label, onchange } = $props()
  function keys(event) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const index = tabs.findIndex(tab => tab.id === value)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length
    onchange(tabs[next].id)
    event.currentTarget.querySelectorAll('[role=tab]')[next]?.focus()
  }
</script>
<div class="tabs" role="tablist" aria-label={label} onkeydown={keys}>
  {#each tabs as tab (tab.id)}
    <button type="button" role="tab" aria-label={`${tab.label}${tab.count ? ` (${tab.count})` : ''}`} aria-selected={value === tab.id} tabindex={value === tab.id ? 0 : -1} onclick={() => onchange(tab.id)}>
      {#if tab.icon === 'mcp'}<McpIcon />{:else if tab.icon}<LucideIcon name={tab.icon} size={16} />{/if}{tab.label}{#if tab.count} <span>({tab.count})</span>{/if}
    </button>
  {/each}
</div>
<style>
  .tabs { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  button { display: inline-flex; align-items: center; gap: 8px; border: 0; border-radius: var(--radius-pill); padding: 6px 12px; background: transparent; color: var(--muted); font: var(--text-13) var(--font-human); cursor: pointer; min-height: 30px; }
  button[aria-selected=true] { background: var(--signal-soft); color: var(--signal); }
  button:hover { background: var(--faint); }
  button:focus-visible { border-color: var(--ink); background: var(--faint); }
  span { font-size: var(--text-12); font-variant-numeric: tabular-nums; }
</style>

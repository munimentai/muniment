<script>
  import icons from './provider-icons.json'
  let { entry, size = 28 } = $props()
  let failed = $state(false)
  const icon = $derived(entry.icon || icons[entry.id]?.path || icons[entry.source?.split('/').at(-1)]?.path)
  $effect(() => { void entry.id; failed = false })
</script>
<span class="logo" style:width={`${size}px`} style:height={`${size}px`} aria-hidden="true">
  {#if icon && !failed}<img src={icon} alt="" width={size - 6} height={size - 6} loading="lazy" onerror={() => failed = true} />{:else}<span>{entry.name.slice(0, 2).toUpperCase()}</span>{/if}
</span>
<style>
  .logo { flex: none; display: inline-flex; align-items: center; justify-content: center; border-radius: var(--radius-control); background: #fff; color: #222; overflow: hidden; font-size: var(--text-12); }
  img { object-fit: contain; }
</style>

<script>
  import { onMount } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { executionTime } from './execution-time.js'

  let { startedAt } = $props()
  const mountedAt = Date.now()
  let now = $state(mountedAt)
  const start = $derived(Number.isFinite(Date.parse(startedAt)) ? Date.parse(startedAt) : mountedAt)
  const elapsed = $derived(executionTime((now - start) / 1000))
  onMount(() => {
    const timer = setInterval(() => { now = Date.now() }, 1000)
    return () => clearInterval(timer)
  })
</script>

<span class="receipt-time message-time" aria-label={`Execution time: ${elapsed}`}><LucideIcon name="clock" variant="action" size={12} />{elapsed}</span>

<style>
  .receipt-time { display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; font-variant-numeric: tabular-nums; }
</style>

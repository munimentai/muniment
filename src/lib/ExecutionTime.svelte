<script>
  import LucideIcon from './LucideIcon.svelte'
  import { executionTime, nextSecondChange } from './execution-time.js'

  // A live reply's running time, rounded as its receipt rounds the recorded
  // time. It updates the moment the rounded second changes, so its last value
  // is the receipt's value.
  let { startedAt } = $props()
  const mountedAt = Date.now()
  let now = $state(mountedAt)
  const start = $derived(Number.isFinite(Date.parse(startedAt)) ? Date.parse(startedAt) : mountedAt)
  const elapsed = $derived(executionTime((now - start) / 1000))
  $effect(() => {
    const from = start
    let timer
    const update = () => {
      now = Date.now()
      timer = setTimeout(update, nextSecondChange(now - from))
    }
    update()
    return () => clearTimeout(timer)
  })
</script>

<span class="receipt-time" aria-label={`Execution time: ${elapsed}`}><LucideIcon name="clock" variant="action" size={12} />{elapsed}</span>

<style>
  .receipt-time { display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; font-variant-numeric: tabular-nums; }
</style>

<script>
  import LucideIcon from './LucideIcon.svelte'
  import { executionTime, nextSecondChange } from './execution-time.js'

  // A live reply's running time from the start the runtime names, which its
  // receipt times from, rounded as the receipt rounds. Before the runtime names
  // that start it shows nothing, so the time never counts back. It updates the
  // moment the rounded second changes, so its last value is the receipt's.
  let { startedAt } = $props()
  let now = $state(Date.now())
  const start = $derived(Date.parse(startedAt))
  const elapsed = $derived(Number.isFinite(start) ? executionTime((now - start) / 1000) : null)
  $effect(() => {
    const from = start
    if (!Number.isFinite(from)) return
    let timer
    const update = () => {
      now = Date.now()
      timer = setTimeout(update, nextSecondChange(now - from))
    }
    update()
    return () => clearTimeout(timer)
  })
</script>

{#if elapsed}<span class="receipt-time" aria-label={`Execution time: ${elapsed}`}><LucideIcon name="clock" variant="action" size={12} />{elapsed}</span>{/if}

<style>
  .receipt-time { display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; font-variant-numeric: tabular-nums; }
</style>

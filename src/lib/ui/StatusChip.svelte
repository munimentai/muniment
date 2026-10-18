<script>
  // One state that core reports, in one of five tones: success for online and
  // done, active for running, danger for a failure, warning for queued and
  // waiting, and neutral for offline and idle. `status-tone.js` maps each
  // reported state to its tone. `dot` alone draws the colored mark with the
  // text for screen readers only, for a dense row.
  let { tone = 'neutral', dot = false, children, ...rest } = $props()
</script>
<span class="status-chip" class:dot data-tone={tone} {...rest}><i aria-hidden="true"></i>{#if dot}<span class="visually-hidden">{@render children?.()}</span>{:else}{@render children?.()}{/if}</span>
<style>
  .status-chip { --tone: var(--muted); display: inline-flex; flex: none; align-items: center; gap: 5px; max-width: 100%; box-sizing: border-box; padding: 0 6px; border-radius: var(--radius-chip); background: color-mix(in srgb, var(--tone) 14%, transparent); color: var(--tone); font: var(--weight-semibold) var(--text-12) var(--font-human); line-height: 18px; white-space: nowrap; }
  .status-chip i { flex: none; width: 6px; height: 6px; border-radius: var(--radius-chip); background: currentColor; }
  .status-chip[data-tone="success"] { --tone: var(--success); }
  .status-chip[data-tone="active"] { --tone: var(--active); }
  .status-chip[data-tone="danger"] { --tone: var(--danger); }
  .status-chip[data-tone="warning"] { --tone: var(--warning); }
  .status-chip.dot { padding: 0; background: none; }
</style>

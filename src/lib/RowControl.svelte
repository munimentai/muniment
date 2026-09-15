<script>
  // The title row's one quiet control. New thread, the thread title and
  // Artifacts render through it, so the three share one padding and one hover.
  let { kind, element = $bindable(), children, ...rest } = $props()
</script>

<button type="button" class="quiet row-control" class:new-thread={kind === 'new-thread'} class:thread-title={kind === 'thread-title'} class:artifacts-toggle={kind === 'artifacts-toggle'} bind:this={element} {...rest}>{@render children()}</button>

<style>
  .row-control { flex: none; display: inline-flex; align-items: center; justify-content: center; gap: 6px; min-width: 24px; min-height: 24px; height: 24px; padding: 0 6px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; cursor: pointer; }
  .quiet { background: transparent; border-color: transparent; }
  .row-control:hover:not(:disabled) { background: var(--faint); border-color: transparent; }
  /* The rename control is the one row control that shows state as a border: the composer's muted hairline. */
  .thread-title:hover:not(:disabled), .thread-title:focus-visible { border-color: var(--muted); background: transparent; }
  .row-control:disabled { color: var(--muted); cursor: default; }
  .new-thread, .artifacts-toggle { white-space: nowrap; }
  /* The title takes the row's remaining width and fades to an ellipsis at its end. */
  .thread-title { max-width: 100%; overflow: hidden; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .thread-title:disabled { color: var(--ink); opacity: 1; }
  /* Artifacts sits after the update slot on macOS, flush right in the row. */
  :global(.workspace.macos) .artifacts-toggle { order: 2; }
</style>

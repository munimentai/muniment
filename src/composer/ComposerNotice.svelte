<script>
  // The composer's short notice: the current notice of a `createComposerNotices`
  // queue, in the composer panels' place above the composer's top edge on the
  // same overlay surface. The pointer over it or keyboard focus in it holds its
  // clock. An open composer panel hides it and holds its clock until it closes.
  // It carries one line, an optional progress bar, an optional action and a
  // close control. `placement: 'end'` sets it at the composer's right edge, over
  // the Voice button. A progress without `max` shows an indeterminate bar, and
  // `alert` announces a failure at once.
  import { onMount, tick } from 'svelte'
  import PopupClose from '../lib/PopupClose.svelte'

  let { notices } = $props()
  let current = $state(null)
  let element = $state()

  const covered = () => !!element?.parentElement?.querySelector('[data-composer-panel]:not([data-composer-notice])')

  onMount(() => {
    const detach = notices.attach(covered)
    const stop = notices.subscribe((notice) => {
      // Focus inside a notice that leaves returns to the composer's field.
      const composer = element?.contains(document.activeElement) ? element.parentElement : null
      current = notice
      if (composer) void tick().then(() => composer.querySelector('textarea')?.focus())
    })
    return () => { stop(); detach() }
  })

  function keyboardFocus(target) {
    try { return target.matches(':focus-visible') } catch { return true }
  }

  function focusin(event) {
    if (keyboardFocus(event.target)) notices.hold('focus')
  }

  function focusout(event) {
    if (!element?.contains(event.relatedTarget)) notices.release('focus')
  }
</script>

{#if current}
  {#key `${current.id}\n${current.cause}`}
    <section
      bind:this={element}
      class="composer-notice"
      class:end={current.placement === 'end'}
      data-composer-notice
      data-composer-panel
      data-panel="composer-notice"
      data-panel-variant="overlay"
      role={current.alert ? 'alert' : 'status'}
      aria-label={current.label}
      data-testid={current.testid}
      onpointerenter={() => notices.hold('pointer')}
      onpointerleave={() => notices.release('pointer')}
      onfocusin={focusin}
      onfocusout={focusout}
    >
      <p>{current.text}</p>
      {#if current.progress}<span class="progress">{#if current.progress.max > 0}<progress aria-label={current.progress.label} max={current.progress.max} value={current.progress.value}></progress>{:else}<progress aria-label={current.progress.label}></progress>{/if}{#if current.progress.text}<span class="amount">{current.progress.text}</span>{/if}</span>{/if}
      {#if current.action}<button type="button" class="action" onclick={() => notices.act()}>{current.action.label}</button>{/if}
      <PopupClose label={current.dismissLabel ?? 'Dismiss notice'} onclick={() => notices.dismiss()} />
    </section>
  {/key}
{/if}

<style>
  .composer-notice { display: flex; align-items: center; gap: 8px; width: max-content; max-width: 100%; box-sizing: border-box; padding: 6px 6px 6px 12px; animation: composer-notice-enter var(--motion-popover) var(--ease-out); }
  .composer-notice.end { left: auto; right: 0; }
  /* An open composer panel takes the shared anchor, so the notice waits under it. */
  :global(:has([data-composer-panel]:not([data-composer-notice]))) > .composer-notice { visibility: hidden; }
  p { flex: 1; min-width: 0; margin: 0; color: var(--ink); font: var(--text-13) / var(--leading-body) var(--font-human); overflow-wrap: anywhere; }
  .progress { flex: none; display: flex; align-items: center; gap: 8px; color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  /* The byte counts keep their width while they change. */
  .amount { font-variant-numeric: tabular-nums; }
  progress { width: 96px; height: 6px; accent-color: var(--muted); }
  .action { flex: none; padding: 5px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; }
  .action:hover { background: var(--faint); }
  @keyframes composer-notice-enter { from { opacity: 0; transform: translateY(2px); } }
</style>

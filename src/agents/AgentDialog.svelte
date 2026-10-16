<script>
  // The dialog the agent screens share: a titled panel on the page body with
  // its close control, a scrolling body and a row of actions. Escape, the X
  // and a click outside close it unless work is pending.
  import { onMount, tick } from 'svelte'
  import { dialogDismiss } from '../lib/dialog-dismiss.js'
  import PopupClose from '../lib/PopupClose.svelte'

  let { title, panel = 'agent-dialog', busy = false, width = 560, onclose, children, actions } = $props()
  let element
  const id = $props.id()

  onMount(() => { void tick().then(() => element?.querySelector('input:not([type="hidden"]), select, textarea, button:not(.popup-close)')?.focus()) })

  function portal(node) {
    document.body.appendChild(node)
    return { destroy() { node.remove() } }
  }

  function keydown(event) {
    if (event.key === 'Escape' && !busy) { event.preventDefault(); event.stopPropagation(); onclose() }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="dialog-backdrop" use:portal use:dialogDismiss={{ onclose, disabled: busy }}>
  <div data-panel={panel} data-panel-variant="overlay" class="dialog-panel" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`} style:--dialog-width={`${width}px`} bind:this={element}>
    <header><h2 id={`${id}-title`}>{title}</h2><PopupClose label={`Close ${title.toLowerCase()}`} disabled={busy} onclick={onclose} /></header>
    <div class="body">{@render children()}</div>
    {#if actions}<div class="dialog-actions">{@render actions()}</div>{/if}
  </div>
</div>

<style>
  .dialog-backdrop { position: fixed; inset: 0; z-index: 10; display: grid; place-items: center; padding: 24px; background: var(--overlay-backdrop); }
  .dialog-panel { display: flex; flex-direction: column; width: min(var(--dialog-width), 100%); max-height: calc(100vh - 48px); font-family: var(--font-human); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 16px 20px 8px; }
  h2 { margin: 0; font-size: var(--text-15); }
  .body { display: grid; gap: 12px; min-height: 0; overflow-y: auto; padding: 8px 20px 12px; }
  .dialog-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 12px 20px 16px; border-top: 1px solid var(--border); }
</style>

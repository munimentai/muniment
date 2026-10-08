<script>
  import { tick } from 'svelte'
  import { dialogDismiss } from './dialog-dismiss.js'
  import PopupClose from './PopupClose.svelte'
  import { receiptRows } from './chat-state.js'

  // A long run routes every turn, so the turns open in an overlay that looks
  // like Settings: a blurred scrim over the window and a panel of the same size
  // that scrolls instead of growing the transcript.
  let { routing = [] } = $props()
  let shown = $state(false)
  let trigger = $state()
  let panel = $state()
  const label = $derived(`Routing details · ${routing.length} ${routing.length === 1 ? 'turn' : 'turns'}`)

  async function open() { shown = true; await tick(); panel?.querySelector('button')?.focus() }
  function close() { shown = false; trigger?.focus() }
  // The overlay mounts on the page body, so no scrolling or clipped transcript
  // ancestor can move or cut it.
  function portal(node) {
    document.body.appendChild(node)
    return { destroy() { node.remove() } }
  }
</script>

<svelte:window onkeydown={(event) => { if (shown && event.key === 'Escape') { event.preventDefault(); close() } }} />
<button type="button" class="routing-trigger" aria-haspopup="dialog" bind:this={trigger} onclick={open}>{label}</button>
{#if shown}
  <div class="routing-scrim" use:portal use:dialogDismiss={{ onclose: close }}>
    <div data-panel data-panel-variant="overlay" class="routing-panel" role="dialog" aria-modal="true" aria-label="Routing details" bind:this={panel}>
      <header data-panel-header class="routing-head">
        <h3>{label}</h3>
        <PopupClose label="Close routing details" onclick={close} />
      </header>
      <div class="routing-turns">
        {#each routing as evidence, index}
          <section aria-label={`Routing turn ${index + 1}`}>
            <h4>Turn {index + 1}</h4>
            <dl>
              {#each receiptRows({ routing: [evidence] }) as row}
                <div><dt>{row.label}</dt><dd>{row.value}</dd></div>
              {/each}
            </dl>
          </section>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .routing-trigger { display: block; margin-top: 10px; padding: 0; border: 0; background: transparent; color: var(--muted); font: var(--text-12) var(--font-mono); cursor: pointer; }
  .routing-trigger:hover, .routing-trigger:focus-visible { color: var(--ink); }
  /* The same scrim, blur and panel size as Settings. */
  .routing-scrim { position: fixed; inset: 0; z-index: 8; display: grid; place-items: center; padding: 40px; background: var(--overlay-backdrop); -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); }
  .routing-panel { display: flex; flex-direction: column; width: min(1080px, 100%); height: min(760px, 100%); overflow: hidden; }
  .routing-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: var(--panel-control-inset) var(--panel-control-inset) var(--panel-control-inset) 28px; border-bottom: 1px solid var(--border); }
  h3 { margin: 0; font-size: var(--text-15); font-weight: 600; }
  .routing-turns { display: grid; align-content: start; gap: 12px; flex: 1; min-height: 0; overflow-y: auto; padding: 20px 28px 28px; }
  section { padding: 14px 16px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  h4 { margin: 0 0 10px; font-size: var(--text-13); font-weight: 600; color: var(--ink); }
  dl { display: grid; row-gap: 8px; margin: 0; }
  dl div { display: grid; grid-template-columns: minmax(0, 180px) minmax(0, 1fr); gap: 16px; }
  dt { color: var(--muted); font-size: var(--text-13); overflow-wrap: anywhere; }
  dd { margin: 0; font: var(--text-12) var(--font-mono); line-height: 1.6; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  @media (max-width: 600px) { .routing-scrim { padding: 16px; } dl div { grid-template-columns: 1fr; gap: 2px; } }
</style>

<script>
  import LucideIcon from '../LucideIcon.svelte'
  // An info control that holds the explanation a title or label leaves out.
  // Hover or keyboard focus shows the text, and a click pins it open until
  // Escape, a second click or focus leaves. The text stays in the document as
  // the tooltip, so the owner's aria-describedby can name `id`.
  let { text = '', label = 'More information', id = '' } = $props()
  const ownId = $props.id()
  const tipId = $derived(id || `${ownId}-tip`)
  let pinned = $state(false)
</script>

{#if text}
  <span class="info-tip" data-open={pinned ? 'true' : undefined}>
    <button type="button" class="info-button" data-ui-button aria-label={label} aria-describedby={tipId} aria-expanded={pinned}
      onclick={() => (pinned = !pinned)}
      onblur={() => (pinned = false)}
      onkeydown={(event) => { if (event.key === 'Escape' && pinned) { event.stopPropagation(); pinned = false } }}>
      <LucideIcon name="info" size={14} variant="action" />
    </button>
    <span role="tooltip" id={tipId} class="info-text">{text}</span>
  </span>
{/if}

<style>
  .info-tip { position: relative; display: inline-flex; flex: none; align-items: center; }
  .info-button { display: inline-grid; place-items: center; width: 24px; height: 24px; padding: 0; color: var(--muted); background: transparent; border: 0; border-radius: var(--radius-control); cursor: pointer; }
  .info-button:hover, .info-button[aria-expanded="true"] { color: var(--ink); background: var(--faint); }
  .info-text { position: absolute; z-index: 8; left: 0; top: calc(100% + 6px); width: max-content; max-width: min(340px, 70vw); padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); color: var(--ink); font: var(--weight-regular) var(--text-13) / var(--leading-body) var(--font-human); white-space: normal; overflow-wrap: anywhere; pointer-events: none; opacity: 0; visibility: hidden; transition: opacity 80ms ease; }
  .info-tip:hover .info-text, .info-button:focus-visible + .info-text, .info-tip[data-open] .info-text { opacity: 1; visibility: visible; }
  .info-tip:hover .info-text { transition-delay: 200ms; }
  :global(:root[data-motion="reduce"]) .info-text { transition: none; }
</style>

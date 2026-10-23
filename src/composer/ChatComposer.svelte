<script>
  // Both regular threads and agent threads render this same composer instance.
  // Snippets retain the shared chat controller bindings and feature controls.
  // The composer is a size container, so a narrow one keeps its menu buttons to icons.
  let { element = $bindable(), children } = $props()
</script>
<div class="composer" bind:this={element}>{@render children()}</div>
<style>
  .composer { container: composer / inline-size; grid-area: 1 / 1; align-self: end; z-index: 1; position: relative; width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  /* The text field leaves its ring to this frame: the border and a 1px outline make the 2px edge without moving the content. */
  .composer:focus-within { border-color: var(--focus-edge); outline: 1px solid var(--focus-edge); }
  /* While the voice wave shows, the wave is the only line on the top edge. The
     frame's own border and outline turn transparent, and an overlay draws the
     edge instead: the sides and bottom in full, each top corner in the edge
     color, fading to transparent along the first 24px of the straight top edge,
     where the wave starts in the same color. */
  .composer { --composer-edge: var(--border); }
  .composer:focus-within { --composer-edge: var(--focus-edge); }
  .composer:global([data-voice-wave]), .composer:global([data-voice-wave]):focus-within { border-color: transparent; outline-color: transparent; }
  .composer:global([data-voice-wave])::before {
    --corner: var(--radius-panel);
    content: ''; position: absolute; inset: -1px; border: 1px solid var(--composer-edge); border-radius: var(--radius-panel); pointer-events: none;
    mask-image: linear-gradient(#000, #000), linear-gradient(to right, #000 var(--corner), transparent calc(var(--corner) + 24px), transparent calc(100% - var(--corner) - 24px), #000 calc(100% - var(--corner)));
    mask-position: 0 var(--corner), 0 0;
    mask-size: 100% calc(100% - var(--corner)), 100% var(--corner);
    mask-repeat: no-repeat;
  }
  /* Focused, the overlay is the 2px edge the border and outline make. */
  .composer:global([data-voice-wave]):focus-within::before { inset: -2px; border-width: 2px; border-radius: var(--radius-panel); }
</style>

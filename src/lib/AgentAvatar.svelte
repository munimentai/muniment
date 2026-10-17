<script>
  import { avatarSvg, avatarFor } from './agent-avatar.js'
  // The one agent avatar. It is decorative: the agent's name always sits beside it.
  let { agent, size = 24 } = $props()
</script>
<!-- The generator draws fixed local geometry and never inserts user text. -->
<span class="agent-avatar" aria-hidden="true" style:width="{size}px" style:height="{size}px">{@html avatarSvg(avatarFor(agent))}</span>
<style>
  /* A fixed square box that centers in its slot. The drawing fills it with no baseline gap,
     and a hairline keeps a pale tile on a pale panel, or a dark one on a dark panel, in view.
     The doubled class outranks a parent's `> span` rule, such as the catalog card's text clamp. */
  .agent-avatar.agent-avatar { display: block; flex: none; align-self: center; box-sizing: border-box; overflow: hidden; border-radius: var(--radius-control); outline: 1px solid var(--border); outline-offset: -1px; line-height: 0; }
  .agent-avatar.agent-avatar :global(svg) { display: block; width: 100%; height: 100%; }
</style>

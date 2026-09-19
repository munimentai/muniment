<script>
  import { avatarSvg, avatarFor } from './agent-avatar.js'
  let { agent, size = 24 } = $props()
</script>
<!-- The generator returns fixed local geometry. It never inserts user text. -->
<span class="avatar" aria-hidden="true" style:width="{size}px" style:height="{size}px">{@html avatarSvg(avatarFor(agent))}</span>
<style>.avatar { display: block; flex: none; } .avatar :global(svg) { display: block; width: 100%; height: 100%; }</style>

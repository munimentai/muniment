<script>
  import { overflowFade } from './overflow-fade.js'
  let { text = '', side = 'left', scroll = true } = $props()
</script>
<span class="overflow-text fade-viewport" class:scroll use:overflowFade={{end: side === 'left', text}} title={text} tabindex={scroll ? 0 : undefined}>{text}</span>
<style>
  .overflow-text { display:block; min-width:0; max-width:100%; flex:1; white-space:nowrap; overflow:hidden; }
  .scroll { overflow-x:auto; }
  :global(.fade-viewport) { scrollbar-width:none; --fade-left:0px; --fade-right:0px; mask-image:linear-gradient(to right, transparent, #000 var(--fade-left), #000 calc(100% - var(--fade-right)), transparent); }
  :global(.fade-viewport::-webkit-scrollbar) { display:none; }
  :global(.fade-viewport[data-fade-left="true"]) { --fade-left:28px; }
  :global(.fade-viewport[data-fade-right="true"]) { --fade-right:28px; }
</style>

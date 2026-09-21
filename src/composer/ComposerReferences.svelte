<script>
  import { composerParts } from './composer-references.js'
  let { text = '', references = [], commands = [], scrollTop = 0, scrollLeft = 0, width } = $props()
  const parts = $derived(composerParts(text, references, commands))
</script>
<div class="reference-overlay" aria-hidden="true"><div style:width={width ? `${width}px` : undefined} style:transform={`translate(${-scrollLeft}px, ${-scrollTop}px)`}>{#each parts as part, i (i)}<span class:reference={part.type !== 'text'}>{part.text}</span>{/each}{'\n'}</div></div>
<style>
  .reference-overlay { position: absolute; inset: 0; pointer-events: none; overflow: hidden; color: var(--ink); white-space: pre-wrap; overflow-wrap: break-word; font: inherit; line-height: inherit; }
  .reference { color: var(--reference); }
  @media (forced-colors: active) { .reference { color: LinkText; } }
</style>

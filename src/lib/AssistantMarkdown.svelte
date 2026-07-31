<script>
  import { renderAssistantMarkdown } from './assistant-markdown.js'

  let { text = '' } = $props()
  let rendered = $derived(renderAssistantMarkdown(text))
</script>

<div class="assistant-markdown">
  {#if rendered.kind === 'html'}
    {@html rendered.html}
  {:else}{rendered.text}{/if}
</div>

<style>
  .assistant-markdown { max-width: 92%; color: var(--ink); white-space: pre-wrap; overflow-wrap: anywhere; }
  .assistant-markdown :global(p) { margin: 0 0 12px; }
  .assistant-markdown :global(p:last-child) { margin-bottom: 0; }
  .assistant-markdown :global(h2) { margin: 24px 0 8px; font-size: var(--text-22); }
  .assistant-markdown :global(h3) { margin: 20px 0 7px; font-size: var(--text-17); }
  .assistant-markdown :global(h4), .assistant-markdown :global(h5) { margin: 16px 0 6px; font-size: var(--text-15); }
  .assistant-markdown :global(h2:first-child), .assistant-markdown :global(h3:first-child), .assistant-markdown :global(h4:first-child), .assistant-markdown :global(h5:first-child) { margin-top: 0; }
  .assistant-markdown :global(ul), .assistant-markdown :global(ol) { margin: 0 0 12px; padding-left: 24px; }
  .assistant-markdown :global(li > p) { margin-bottom: 4px; }
  .assistant-markdown :global(blockquote) { margin: 0 0 12px; padding-left: 12px; border-left: 1px solid var(--border); color: var(--muted); }
  .assistant-markdown :global(hr) { margin: 20px 0; border: 0; border-top: 1px solid var(--border); }
  .assistant-markdown :global(a) { color: var(--ink); text-decoration: underline; text-decoration-color: var(--border); text-underline-offset: 2px; }
  .assistant-markdown :global(a:hover) { color: var(--muted); text-decoration-color: var(--muted); }
  .assistant-markdown :global(code) { padding: 1px 3px; border-radius: var(--radius-chip); background: var(--faint); font: var(--text-13) var(--font-mono); }
  .assistant-markdown :global(pre) { max-width: 100%; margin: 0 0 12px; overflow-x: auto; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); }
  .assistant-markdown :global(pre code) { display: block; width: max-content; min-width: 100%; padding: 10px 12px; border-radius: var(--radius-control); background: var(--faint); white-space: pre; }
  .assistant-markdown :global(table) { display: block; max-width: 100%; margin: 0 0 12px; overflow-x: auto; border-collapse: collapse; white-space: nowrap; }
  .assistant-markdown :global(th), .assistant-markdown :global(td) { padding: 6px 10px; border: 1px solid var(--border); text-align: left; }
  .assistant-markdown :global(th) { background: var(--faint); font-weight: var(--weight-semibold); }
</style>

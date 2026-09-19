<script>
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { highlightFile } from './file-highlight.js'
  let { file, tauri, maximized = false, ontogglemaximized, onclose } = $props()
  let content = $state('')
  let error = $state('')
  let loading = $state(false)
  let reload = $state(0)
  const highlighted = $derived(highlightFile(file?.path ?? '', content))
  $effect(() => {
    const path = file?.path
    void reload
    let current = true
    content = ''; error = ''; loading = !!path
    if (path) tauri.invoke('chat_file_content', { path }).then((text) => { if (current) content = text }).catch((failure) => { if (current) error = String(failure?.message ?? failure) }).finally(() => { if (current) loading = false })
    return () => { current = false }
  })
</script>

<aside id="file-panel" class="file-panel" aria-labelledby="file-panel-title">
  <header>
    <h2 id="file-panel-title">{file?.name ?? 'File'}</h2>
    <button type="button" aria-label="Reload file" onclick={() => reload += 1}><LucideIcon name="refresh-ccw-dot" /></button>
    <button type="button" onclick={ontogglemaximized}>{maximized ? 'Restore' : 'Maximize'}</button>
    <button type="button" aria-label="Close file panel" onclick={onclose}><LucideIcon name="x" /></button>
  </header>
  <p class="path">{file?.path}</p>
  {#if loading}<p role="status">Reading file…</p>
  {:else if error}<p role="alert">{error}</p>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre role="region" tabindex="0" aria-label="File contents"><code>{@html highlighted}</code></pre>{/if}
</aside>

<style>
  .file-panel { grid-area: rail; display: flex; flex-direction: column; min-width: 0; min-height: 0; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); overflow: hidden; }
  header { display: flex; align-items: center; gap: 6px; padding: 12px; border-bottom: 1px solid var(--border); }
  h2 { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; margin: 0; font: var(--text-13) var(--font-mono); }
  button { display: flex; align-items: center; min-height: 28px; padding: 4px 6px; color: var(--ink); background: transparent; border: 1px solid var(--border); border-radius: var(--radius-control); font: inherit; font-size: var(--text-12); cursor: pointer; }
  button:hover { background: var(--faint); }
  p { margin: 12px; color: var(--muted); }
  .path { overflow-wrap: anywhere; font: var(--text-12) var(--font-mono); }
  pre { flex: 1; overflow: auto; margin: 0; padding: 12px; color: var(--ink); background: var(--paper); tab-size: 4; font: var(--text-13)/1.6 var(--font-mono); }
  pre :global(.hljs-comment), pre :global(.hljs-quote) { color: var(--muted); }
  pre :global(.hljs-keyword), pre :global(.hljs-selector-tag), pre :global(.hljs-literal) { color: var(--oxide); }
  pre :global(.hljs-string), pre :global(.hljs-attr), pre :global(.hljs-addition) { color: var(--code-added); }
  pre :global(.hljs-number), pre :global(.hljs-type), pre :global(.hljs-title), pre :global(.hljs-built_in) { color: var(--code-name); }
</style>

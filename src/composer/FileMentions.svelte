<script>
  import FileIcon from '../files/FileIcon.svelte'
  let { rootLabel = 'Muniment folder', files = [], loading = false, error = '', selected = 0, onchoose } = $props()
</script>
<div class="mentions">
  <p>Files in {rootLabel}</p>
  {#if loading}<p role="status">Searching files…</p>
  {:else if error}<p role="alert">{error}</p>
  {:else if !files.length}<p role="status">No matching files. Choose another folder in Files.</p>
  {/if}
  <div id="file-mentions" role="listbox" aria-label="File suggestions">
    {#each files as file, i (file.path)}
      <button id={`file-mention-${i}`} type="button" role="option" aria-selected={selected === i} onpointerdown={(event) => event.preventDefault()} onclick={() => onchoose(file)}><FileIcon name={file.displayName || file.relativePath} /><span>{file.relativePath}</span></button>
    {/each}
  </div>
</div>
<style>
  .mentions { position: absolute; bottom: calc(100% + 8px); left: 0; right: 0; max-height: 280px; overflow: auto; padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); box-shadow: var(--shadow-overlay); z-index: 8; }
  p { margin: 4px 6px 8px; color: var(--muted); font-size: var(--text-12); }
  button { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 32px; padding: 6px; border: 0; text-align: left; background: transparent; color: var(--ink); font: var(--text-13) var(--font-mono); cursor: pointer; }
  button[aria-selected="true"], button:hover { background: var(--faint); }
  span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
</style>

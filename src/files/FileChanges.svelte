<script>
  let { files = [], onopen } = $props()
  let open = $state(false)
  let trigger = $state()
  const additions = $derived(files.reduce((n, f) => n + f.additions, 0))
  const deletions = $derived(files.reduce((n, f) => n + f.deletions, 0))
  const incomplete = $derived(files.some((f) => f.incomplete))
  // Line counts when every file has them, else file counts: changed files in green, deleted files in red.
  const removedFiles = $derived(files.filter((f) => f.deleted).length)
  const noun = (n, one, many) => `${n} ${n === 1 ? one : many}`
  const label = $derived(`${noun(files.length, 'file', 'files')} changed, ${incomplete
    ? `${noun(files.length - removedFiles, 'file', 'files')} added or edited, ${noun(removedFiles, 'file', 'files')} deleted`
    : `${noun(additions, 'line', 'lines')} added, ${noun(deletions, 'line', 'lines')} removed`}`)
</script>

{#if files.length}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="changes" onpointerenter={(event) => { if (event.pointerType !== 'touch') open = true }} onpointerleave={() => { if (!document.activeElement?.closest('.changes')) open = false }} onfocusout={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) open = false }} onkeydown={(event) => { if (event.key === 'Escape') { open = false; trigger?.focus(); event.stopPropagation() } }} role="group" aria-label="Changed files">
    <button bind:this={trigger} class="chip" type="button" aria-label={label} aria-expanded={open} onclick={() => open = true}>{#if incomplete}<span class="added">+{noun(files.length - removedFiles, 'file', 'files')}</span><span class="removed">−{noun(removedFiles, 'file', 'files')}</span>{:else}<span>{noun(files.length, 'file', 'files')}</span><span class="added">+{additions}</span><span class="removed">−{deletions}</span>{/if}</button>
    {#if open}
      <div class="file-menu">
        {#each files as file (file.path)}
          <button type="button" aria-label={file.path} onclick={() => { open = false; onopen?.(file) }}><span>{file.name}</span>{#if !file.incomplete}<span class="added">+{file.additions}</span><span class="removed">−{file.deletions}</span>{/if}</button>
        {/each}
        {#if incomplete}<p>Some line counts are unavailable.</p>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .changes { position: relative; width: max-content; max-width: 100%; margin: 0; }
  button { color: var(--ink); background: var(--faint); border: 1px solid var(--border); font: inherit; font-size: var(--text-13); cursor: pointer; }
  .chip { display: flex; align-items: center; gap: 6px; height: 32px; padding: 0 12px; border-radius: var(--radius-chip); font-family: var(--font-mono); font-size: var(--text-12); }
  .file-menu { position: absolute; bottom: 100%; left: 50%; transform: translateX(-50%); padding: 6px; width: min(420px, 75vw); max-height: 300px; overflow: auto; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .file-menu button { display: flex; gap: 8px; width: 100%; padding: 9px; border: 0; background: transparent; text-align: left; }
  .file-menu button:hover, .file-menu button:focus-visible { background: var(--faint); }
  .file-menu button span:first-child { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file-menu p { color: var(--muted); margin: 6px; font-size: var(--text-12); }
  .added { color: var(--code-added); }
  .removed { color: var(--oxide); }
</style>

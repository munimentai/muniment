<script>
  import { onMount, tick } from 'svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import PopupClose from '../lib/PopupClose.svelte'
  import { dialogDismiss } from '../lib/dialog-dismiss.js'
  import { irreversibleLines, rewindFiles } from './run-trust.js'

  // Before a rewind runs, core says exactly what it restores and what it
  // cannot take back. The person picks the conversation alone, or the
  // conversation and the files.
  let { tauri, runtime, threadId, runId, prompt = '', ondone, onclose } = $props()

  let preview = $state(null)
  let failure = $state('')
  let pending = $state(false)
  let cancel = $state()

  const files = $derived(preview ? rewindFiles(preview.files) : null)
  const effects = $derived(preview ? irreversibleLines(preview.irreversible) : [])
  const changesFiles = $derived(!!files?.available && (files.restore.length > 0 || files.remove.length > 0))
  const blocked = $derived(preview?.blocked ?? (preview && !preview.available ? preview.unavailable : null))
  const turns = $derived(Number(preview?.turns) || 1)

  async function load() {
    try {
      preview = await agentRequest(tauri, runtime, 'thread.rewind_preview', { thread_id: threadId, run_id: runId })
    } catch (error) {
      failure = String(error?.message ?? error)
    }
  }

  onMount(() => {
    void tick().then(() => cancel?.focus())
    void load()
  })

  async function rewind(withFiles) {
    if (pending) return
    pending = true
    failure = ''
    try {
      const body = { thread_id: threadId, run_id: runId }
      if (withFiles) body.token = preview.files.token
      const result = await agentRequest(tauri, runtime, 'thread.rewind', body)
      ondone?.(result)
    } catch (error) {
      failure = String(error?.message ?? error)
      // The folder may have changed: read what a rewind would do now.
      await load()
    } finally {
      pending = false
    }
  }

  function portal(node) {
    document.body.appendChild(node)
    return { destroy() { node.remove() } }
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === 'Escape' && !pending) { event.preventDefault(); onclose?.() } }} />
<div class="rewind-backdrop" use:portal use:dialogDismiss={{ onclose: () => onclose?.(), disabled: pending }}>
  <div data-panel="rewind" data-panel-variant="overlay" class="rewind-panel" role="dialog" aria-modal="true" aria-labelledby="rewind-title">
    <header>
      <h2 id="rewind-title">Rewind to before this message</h2>
      <PopupClose label="Close rewind" disabled={pending} onclick={() => onclose?.()} />
    </header>
    <div class="rewind-copy">
      {#if prompt}<p class="prompt">{prompt}</p>{/if}
      {#if blocked}<p class="caution" role="alert"><LucideIcon name="triangle-alert" size={14} />{blocked}</p>{/if}
      {#if failure}<p class="caution" role="alert"><LucideIcon name="triangle-alert" size={14} />{failure}</p>{/if}
      {#if !preview && !failure}<p role="status">Checking what a rewind restores.</p>{/if}
      {#if preview}
        <p>The conversation returns to before this message and drops {turns === 1 ? 'this turn' : `this turn and ${turns - 1} later ${turns === 2 ? 'turn' : 'turns'}`}. The message goes back to the composer.</p>
        <section aria-labelledby="rewind-files">
          <h3 id="rewind-files">Files</h3>
          {#if files.note}<p>{files.note}</p>{/if}
          {#if files.restore.length}<p>Restores {files.restore.length} {files.restore.length === 1 ? 'file' : 'files'}:</p><ul>{#each files.restore as path}<li>{path}</li>{/each}</ul>{/if}
          {#if files.remove.length}<p>Removes {files.remove.length} {files.remove.length === 1 ? 'file' : 'files'} made since:</p><ul>{#each files.remove as path}<li>{path}</li>{/each}</ul>{/if}
          {#if files.cannot.length}<p class="caution"><LucideIcon name="triangle-alert" size={14} />Cannot restore:</p><ul>{#each files.cannot as item}<li>{item.path} <span>{item.reason}</span></li>{/each}</ul>{/if}
          {#if files.notCovered.length}<p class="quiet">Not covered, left as they are: <span class="mono">{files.notCovered.join(', ')}</span></p>{/if}
        </section>
        <section aria-labelledby="rewind-effects">
          <h3 id="rewind-effects">Cannot be undone</h3>
          {#if effects.length}
            {#each effects as effect (effect.kind)}
              <p>{effect.headline} {effect.note}</p>
              {#if effect.labels.length}<ul>{#each effect.labels as label}<li>{label}</li>{/each}</ul>{/if}
            {/each}
          {:else}
            <p>Nothing these turns did reached outside the folder.</p>
          {/if}
        </section>
      {/if}
    </div>
    <div class="rewind-actions">
      <button bind:this={cancel} disabled={pending} onclick={() => onclose?.()}>Cancel</button>
      {#if preview && changesFiles}
        <button disabled={pending || !!blocked} onclick={() => rewind(false)}>Rewind conversation only</button>
        <button class="primary" disabled={pending || !!blocked} onclick={() => rewind(true)}>Rewind conversation and files</button>
      {:else if preview}
        <button class="primary" disabled={pending || !!blocked} onclick={() => rewind(false)}>Rewind conversation</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .rewind-backdrop { position: fixed; inset: 0; z-index: 10; display: grid; place-items: center; padding: 24px; background: var(--overlay-backdrop); }
  .rewind-panel { display: flex; flex-direction: column; width: min(560px, 100%); max-height: min(720px, 100%); padding: 24px; font-family: var(--font-human); }
  header { display: flex; align-items: start; justify-content: space-between; gap: 16px; }
  h2 { margin: 0; font-size: var(--text-22); line-height: var(--leading-heading); letter-spacing: var(--tracking-heading); }
  h3 { margin: 16px 0 6px; color: var(--ink); font-size: var(--text-13); font-weight: 600; }
  .rewind-copy { min-height: 0; margin-top: 12px; overflow-y: auto; color: var(--muted); font-size: var(--text-13); line-height: var(--leading-body); overflow-wrap: anywhere; }
  .rewind-copy p { margin: 0 0 6px; }
  .prompt { padding: 8px 12px; border-radius: var(--radius-panel); background: var(--faint); color: var(--ink); white-space: pre-wrap; max-height: 96px; overflow: hidden; }
  ul { margin: 0 0 8px; padding-left: 18px; color: var(--ink); font: var(--text-12)/1.6 var(--font-mono); }
  li span { color: var(--muted); font-family: var(--font-human); }
  .mono { font-family: var(--font-mono); }
  .caution { color: var(--ink); }
  .caution { display: flex; align-items: center; gap: 6px; }
  .caution :global(.lucide) { color: var(--warning); }
  .quiet { font-size: var(--text-12); }
  .rewind-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-top: 20px; }
  button { min-height: 36px; padding: 7px 14px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--weight-semibold) var(--text-13) var(--font-human); cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { opacity: .55; cursor: default; }
  .primary { border-color: var(--accent); background: var(--accent); color: var(--on-accent); }
  .primary:hover:not(:disabled) { opacity: .9; background: var(--accent); }
</style>

<script>
  import { onMount, tick } from 'svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import PopupClose from '../lib/PopupClose.svelte'
  import { dialogDismiss } from '../lib/dialog-dismiss.js'
  import { turnFileState } from '../activity/run-trust.js'

  // Every turn's file changes, whichever tool made them, read from the
  // turn's checkpoints in core. A file or one hunk of it goes back as it was
  // before the turn while the file still holds the turn's change.
  let { tauri, runtime, threadId, turns = [], initialRunId = null, onclose } = $props()

  let changes = $state({})
  let selected = $state(null)
  let pending = $state(false)
  let status = $state('')
  let failure = $state('')
  let panel = $state()

  const visible = $derived(turns.filter((turn) => {
    const entry = changes[turn.runId]
    return !entry || entry.error || entry.value?.files?.length || turn.runId === initialRunId
  }))
  const loading = $derived(turns.some((turn) => !changes[turn.runId]))
  const unsaved = $derived(turns.filter((turn) => changes[turn.runId]?.value?.available === false && turn.runId !== initialRunId).length)
  const selectedTurn = $derived(selected ? changes[selected.runId]?.value ?? null : null)
  const selectedIndex = $derived(selectedTurn ? selectedTurn.files.findIndex((file) => file.path === selected.path) : -1)
  const selectedFile = $derived(selectedIndex >= 0 ? selectedTurn.files[selectedIndex] : null)
  const selectedDiff = $derived(selectedIndex >= 0 ? selectedTurn.diff?.files?.[selectedIndex] ?? null : null)

  async function load(runId) {
    try {
      const value = await agentRequest(tauri, runtime, 'run.changes', { thread_id: threadId, run_id: runId })
      changes[runId] = { value }
      if (!selected && value.files?.length && (!initialRunId || runId === initialRunId)) selected = { runId, path: value.files[0].path }
    } catch (error) {
      changes[runId] = { error: String(error?.message ?? error) }
    }
  }

  onMount(() => {
    void tick().then(() => panel?.querySelector('button')?.focus())
    const order = initialRunId ? [initialRunId, ...turns.map((turn) => turn.runId).filter((id) => id !== initialRunId)] : turns.map((turn) => turn.runId)
    void (async () => { for (const runId of order) await load(runId) })()
  })

  async function revert(runId, path, hunk = null) {
    if (pending) return
    pending = true
    status = ''
    failure = ''
    try {
      const body = { thread_id: threadId, run_id: runId, path }
      if (hunk !== null) body.hunk = hunk
      const answer = await agentRequest(tauri, runtime, 'run.revert', body)
      changes[runId] = { value: answer.changes }
      status = hunk === null ? `Reverted ${path}.` : `Reverted a change in ${path}.`
    } catch (error) {
      failure = String(error?.message ?? error)
    } finally {
      pending = false
    }
  }

  const sign = (kind) => kind === 'addition' ? '+' : kind === 'deletion' ? '−' : ' '
  function portal(node) {
    document.body.appendChild(node)
    return { destroy() { node.remove() } }
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === 'Escape' && !pending) { event.preventDefault(); onclose?.() } }} />
<div class="review-backdrop" use:portal use:dialogDismiss={{ onclose: () => onclose?.(), disabled: pending }}>
  <div data-panel="review" data-panel-variant="overlay" class="review-panel" role="dialog" aria-modal="true" aria-labelledby="review-title" bind:this={panel}>
    <header class="review-head">
      <h2 id="review-title">Review changes</h2>
      <PopupClose label="Close review" disabled={pending} onclick={() => onclose?.()} />
    </header>
    <div class="review-body">
      <nav class="turns" aria-label="Turns">
        {#each visible as turn (turn.runId)}
          {@const entry = changes[turn.runId]}
          <section class="turn" aria-label={`Turn: ${turn.prompt || 'Message'}`}>
            <h3>{turn.prompt || 'Message'}</h3>
            {#if !entry}<p class="note">Reading the turn's changes.</p>
            {:else if entry.error}<p class="note" role="alert">{entry.error}</p>
            {:else if !entry.value.available}<p class="note">{entry.value.unavailable}</p>
            {:else if !entry.value.files.length}<p class="note">No file changed in this turn.</p>
            {:else}
              <ul>
                {#each entry.value.files as file (file.path)}
                  <li><button type="button" class="file" aria-current={selected?.runId === turn.runId && selected?.path === file.path} onclick={() => { selected = { runId: turn.runId, path: file.path }; status = ''; failure = '' }}>
                    <span class="path">{file.path}</span>
                    {#if file.additions !== undefined}<span class="added">+{file.additions}</span><span class="removed">−{file.deletions}</span>{:else}<span class="counts">{file.binary ? 'binary' : file.status}</span>{/if}
                    {#if turnFileState(file.state)}<span class="state">{turnFileState(file.state)}</span>{/if}
                  </button></li>
                {/each}
              </ul>
              {#if entry.value.more}<p class="note">{entry.value.more} more {entry.value.more === 1 ? 'file' : 'files'} changed.</p>{/if}
            {/if}
          </section>
        {/each}
        {#if !loading && unsaved}<p class="note">{unsaved} earlier {unsaved === 1 ? 'turn has' : 'turns have'} no saved changes to show.</p>
        {:else if !loading && !visible.length}<p class="note">No turn in this thread changed a file.</p>{/if}
      </nav>
      <div class="diff" role="region" aria-label="Diff">
        {#if selectedFile}
          <div class="diff-head">
            <strong class="path">{selectedFile.path}</strong>
            {#if turnFileState(selectedFile.state)}<span class="state">{turnFileState(selectedFile.state)}</span>{/if}
            <button type="button" disabled={pending || selectedFile.state !== 'applied'} onclick={() => revert(selected.runId, selectedFile.path)}>{selectedFile.status === 'added' ? 'Remove file' : 'Revert file'}</button>
          </div>
          {#if status}<p class="status" role="status">{status}</p>{/if}
          {#if failure}<p class="status failure" role="alert">{failure}</p>{/if}
          {#if selectedFile.state === 'changed_since'}<p class="note">This file changed after the turn. Hunks the turn wrote that still stand can revert.</p>{/if}
          {#if selectedDiff?.hunks?.length}
            {#each selectedDiff.hunks as hunk, index (index)}
              <section class="hunk" aria-label={`Change ${index + 1}`}>
                <div class="hunk-head"><span>{hunk.header}</span>
                  {#if selectedFile.status === 'modified'}
                    {#if selectedFile.hunks_applied?.[index]}<button type="button" disabled={pending} onclick={() => revert(selected.runId, selectedFile.path, index)}>Revert change</button>{:else}<span class="state">Reverted</span>{/if}
                  {/if}
                </div>
                <table>
                  <tbody>
                    {#each hunk.lines as line, lineIndex (lineIndex)}
                      <tr class:addition={line.kind === 'addition'} class:deletion={line.kind === 'deletion'}><td class="number">{line.oldLineNumber ?? ''}</td><td class="number">{line.newLineNumber ?? ''}</td><td class="sign">{sign(line.kind)}</td><td class="text">{line.text}</td></tr>
                    {/each}
                  </tbody>
                </table>
              </section>
            {/each}
            {#if selectedTurn.diff.truncated}<p class="note">Some changes are too long to show here.</p>{/if}
          {:else}
            <p class="note">{selectedFile.binary ? 'This file is not text, so its lines are not shown.' : selectedFile.additions === undefined ? 'This file is too large to compare line by line.' : 'No line changed.'}</p>
          {/if}
        {:else if !loading}
          <p class="note center"><LucideIcon name="file-text" size={14} />Pick a file to see its changes.</p>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .review-backdrop { position: fixed; inset: 0; z-index: 9; display: grid; place-items: center; padding: 40px; background: var(--overlay-backdrop); }
  .review-panel { display: flex; flex-direction: column; width: min(1080px, 100%); height: min(760px, 100%); overflow: hidden; font-family: var(--font-human); }
  .review-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: var(--panel-control-inset) var(--panel-control-inset) var(--panel-control-inset) 24px; border-bottom: 1px solid var(--border); }
  h2 { margin: 0; font-size: var(--text-15); font-weight: 600; }
  .review-body { display: grid; grid-template-columns: minmax(220px, 320px) minmax(0, 1fr); flex: 1; min-height: 0; }
  .turns { overflow-y: auto; padding: 12px; border-right: 1px solid var(--border); }
  .turn + .turn { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border); }
  h3 { margin: 0 0 6px; overflow: hidden; color: var(--ink); font-size: var(--text-13); font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  ul { margin: 0; padding: 0; list-style: none; }
  .file { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 28px; padding: 4px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); text-align: left; cursor: pointer; }
  .file:hover, .file[aria-current="true"] { background: var(--faint); }
  .file .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .added { color: var(--code-added); }
  .removed { color: var(--oxide); }
  .state, .counts { color: var(--muted); font: var(--text-12) var(--font-human); }
  .note { margin: 4px 0; color: var(--muted); font-size: var(--text-12); }
  .center { display: flex; align-items: center; gap: 6px; justify-content: center; height: 100%; }
  .diff { overflow: auto; padding: 16px 20px 24px; }
  .diff-head { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; }
  .diff-head .path { flex: 1; min-width: 0; overflow-wrap: anywhere; font: var(--weight-semibold) var(--text-13) var(--font-mono); }
  button:not(.file) { min-height: 28px; padding: 4px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-12) var(--font-human); cursor: pointer; }
  button:not(.file):hover:not(:disabled) { background: var(--faint); }
  button:disabled { opacity: .55; cursor: default; }
  .status { margin: 0 0 10px; font-size: var(--text-12); }
  .failure { color: var(--danger); }
  .hunk { margin-top: 12px; border: 1px solid var(--border); border-radius: var(--radius-control); overflow: hidden; }
  .hunk-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 4px 6px 4px 10px; background: var(--faint); color: var(--muted); font: var(--text-12) var(--font-mono); }
  table { width: 100%; border-collapse: collapse; font: var(--text-12)/1.55 var(--font-mono); }
  td { padding: 0 8px; vertical-align: top; white-space: pre-wrap; overflow-wrap: anywhere; }
  .number { width: 1%; color: var(--muted); text-align: right; white-space: nowrap; user-select: none; }
  .sign { width: 1%; padding: 0 4px; user-select: none; }
  tr.addition { background: color-mix(in srgb, var(--code-added) 10%, transparent); }
  tr.addition .sign, tr.addition .text { color: var(--code-added); }
  tr.deletion { background: color-mix(in srgb, var(--oxide) 10%, transparent); }
  tr.deletion .sign, tr.deletion .text { color: var(--oxide); }
  @media (max-width: 720px) {
    .review-backdrop { padding: 16px; }
    .review-body { grid-template-columns: 1fr; grid-template-rows: minmax(120px, 40%) 1fr; }
    .turns { border-right: 0; border-bottom: 1px solid var(--border); }
  }
</style>

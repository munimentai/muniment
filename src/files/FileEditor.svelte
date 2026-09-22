<script>
  import { onMount } from 'svelte'
  import ConfirmDialog from '../lib/ConfirmDialog.svelte'
  let { file, tauri, threadId, ondirty = () => {} } = $props()
  let host
  let editor, model, baseline = '', revision = '', path = ''
  let dirty = $state(false), busy = $state(true), error = $state(''), ready = $state(false), reloadPrompt = $state(false)
  function changed() { dirty = model.getValue(undefined, true) !== baseline; ondirty(dirty) }
  async function save() {
    if (!model || busy || !dirty) return
    busy = true; error = ''
    const content = model.getValue(undefined, true)
    try {
      const result = await tauri.invoke('workspace_save_text', {path, content, revision})
      baseline = content; revision = result.revision; changed()
    } catch(e) { error = String(e) } finally { busy = false }
  }
  async function reload() {
    reloadPrompt = false; busy = true; error = ''
    try {
      const result = await tauri.invoke('workspace_read_text', {path, threadId})
      baseline = result.content; revision = result.revision
      model.setValue(baseline); changed()
    } catch(e) { error = String(e) } finally { busy = false }
  }
  onMount(() => {
    let disposed = false, subscription
    const media = matchMedia('(prefers-color-scheme: dark)')
    let theme = () => {}
    Promise.all([import('./code-editor.js'), tauri.invoke('workspace_read_text', {path:file.path, threadId})]).then(([{monaco}, result]) => {
      if (disposed) return
      baseline = result.content; revision = result.revision; path = result.path
      const uri = monaco.Uri.from({scheme:'muniment', path, query:crypto.randomUUID()})
      model = monaco.editor.createModel(baseline, undefined, uri)
      editor = monaco.editor.create(host, {model, automaticLayout:true, fontFamily:'CommitMono, monospace', fontSize:13, minimap:{enabled:false}, scrollBeyondLastLine:false, padding:{top:12}, scrollbar:{verticalScrollbarSize:3,horizontalScrollbarSize:3}, ariaLabel:`Edit ${file.name}`})
      theme = () => monaco.editor.setTheme(media.matches ? 'muniment-vs-dark' : 'muniment-vs')
      theme(); media.addEventListener('change', theme)
      subscription = model.onDidChangeContent(changed)
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, save)
      ready = true
    }).catch(e => { if (!disposed) error = String(e) }).finally(() => { if (!disposed) busy = false })
    return () => { disposed = true; subscription?.dispose(); media.removeEventListener('change',theme); editor?.dispose(); model?.dispose() }
  })
</script>
<section class="file-editor" aria-label={`Editor for ${file.name}`}>
  <header><span>{file.path}{dirty ? ' • Unsaved' : ''}</span><button type="button" disabled={!ready || busy} onclick={() => dirty ? reloadPrompt = true : reload()}>Reload</button><button type="button" disabled={!dirty || busy} onclick={save}>Save</button></header>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if busy}<p role="status">{ready ? 'Saving file…' : 'Opening file…'}</p>{/if}
  <div class="editor" bind:this={host}></div>
</section>
{#if reloadPrompt}<ConfirmDialog title="Discard unsaved changes?" cancelLabel="Cancel" confirmLabel="Reload" onDecision={yes => yes ? reload() : reloadPrompt = false}><p>Reload the file from disk and discard your edits.</p></ConfirmDialog>{/if}
<style>
  .file-editor { display:flex; flex-direction:column; flex:1; min-height:0; min-width:0; }
  header { display:flex; align-items:center; gap:8px; padding:6px 10px; border-bottom:1px solid var(--border); }
  span { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  header,p { font:var(--text-12) var(--font-mono); color:var(--muted); } p { padding:0 12px; }
  button { padding:5px 8px; background:var(--surface); color:var(--ink); border:1px solid var(--border); border-radius:var(--radius-control); cursor:pointer; } button:disabled { opacity:.4; }
  .editor { flex:1; min-height:0; min-width:0; }
</style>

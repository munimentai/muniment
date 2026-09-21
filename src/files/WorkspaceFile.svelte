<script>
  import FileEditor from './FileEditor.svelte'
  import FileIcon from './FileIcon.svelte'
  import { fileKind } from './file-kind.js'
  let { file, tauri, threadId = null, ondirty } = $props()
  let src = $state('')
  let error = $state('')
  let loading = $state(false)
  const kind = $derived(fileKind(file.name))
  $effect(() => {
    let current = true
    src = ''; error = ''; loading = kind.icon === 'image'
    if (kind.icon === 'image') tauri.invoke('workspace_image',{path:file.path}).then(value => { if (current) src = value }).catch(e => { if (current) error = String(e) }).finally(() => { if (current) loading = false })
    return () => { current = false }
  })
  async function open() { try { await tauri.invoke('workspace_open',{path:file.path}) } catch(e) { error = String(e) } }
</script>
{#if kind.text}<FileEditor {tauri} {file} {threadId} {ondirty} />
{:else}
  <section class="preview" aria-label={file.name}>
    {#if loading}<p role="status">Reading image…</p>{/if}
    {#if error}<p role="alert">{error}</p>{/if}
    {#if src}<img {src} alt={file.name} />{:else if !loading}<FileIcon name={file.name} />{/if}
    <footer><span>{file.path}</span><button type="button" onclick={open}>Open in default app</button></footer>
  </section>
{/if}
<style>
  .preview { flex:1; min-height:0; display:flex; flex-direction:column; align-items:center; justify-content:center; overflow:auto; padding:12px; gap:12px; }
  img { flex:1; min-height:0; max-width:100%; object-fit:contain; }
  footer { display:flex; align-items:center; gap:12px; width:100%; } span { flex:1; overflow-wrap:anywhere; }
  footer,p { color:var(--muted); font:var(--text-12) var(--font-mono); }
  button { flex:none; padding:5px 8px; background:var(--surface); color:var(--ink); border:1px solid var(--border); border-radius:var(--radius-control); cursor:pointer; }
</style>

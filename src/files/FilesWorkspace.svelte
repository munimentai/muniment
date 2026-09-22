<script>
  import { dialogDismiss } from '../lib/dialog-dismiss.js'
  import PopupClose from '../lib/PopupClose.svelte'
  import OverflowText from '../lib/OverflowText.svelte'
  import { floatingMenu } from '../lib/floating-menu.js'
  import { panelScroll } from '../lib/panel-scroll.js'
  import { onMount, tick } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ConfirmDialog from '../lib/ConfirmDialog.svelte'
  import FileIcon from './FileIcon.svelte'
  import { fileTreeRows } from './file-tree.js'
  let { context: workspaceContext = null, tauri, threadId = null, projectId = null, initialPath = '', onfolder, onfile, hidden = false, onwillchange = () => true, onchanged } = $props()
  let showHidden = $state(false)
  const contextKey = JSON.stringify(workspaceContext ? Object.fromEntries(Object.entries(workspaceContext).filter(([key]) => key !== 'threadTitle')) : {threadId,projectId})
  const storageKey = `muniment.files:${contextKey}`
  let folders = $state([])
  let listing = $state(null)
  let loading = $state(false)
  let error = $state('')
  let selected = $state(null)
  let selection = $state(new Set())
  let clipboard = $state([])
  let menu = $state(null)
  let menuElement
  let naming = $state(null)
  let nameInput
  let proposedName = $state('')
  let deleting = $state(null)
  let busy = $state(false)
  let status = $state('')
  const revealLabel = /Mac/.test(navigator.platform) ? 'Reveal in Finder' : /Win/.test(navigator.platform) ? 'Reveal in File Explorer' : 'Reveal in File Manager'
  const destination = () => selected?.directory ? selected.path : selected?.parent || listing?.path
  function picked() { return [...selection] }
  async function refresh() {
    const root = listing?.path
    if (!root) return
    listing = await tauri.invoke('workspace_list', {path:root})
    const next = {}
    for (const path of expanded) { try { next[path] = (await tauri.invoke('workspace_list',{path})).entries } catch { /* Removed folders leave the tree. */ } }
    children = next
  }
  async function run(action, paths = picked(), name = null, target = destination()) {
    if (busy) return
    if (['rename','trash'].includes(action) && !onwillchange(paths)) { error = 'Save or close the edited files before changing them.'; return }
    busy = true; error = ''; status = ''; menu = null
    try {
      const result = await tauri.invoke('workspace_file_action',{root:listing.path,action,paths,destination:target,name})
      onchanged?.({action,paths,result})
      selection = new Set(); selected = null
      if (target && ['new-file','new-folder','paste'].includes(action) && target !== listing.path) expanded = new Set(expanded).add(target)
      await refresh()
      naming = null; deleting = null
      if (action === 'new-file') onfile?.({path:result[0],name,directory:false})
      status = action === 'trash' ? 'Moved to Trash.' : 'Files updated.'
    } catch(e) { error = String(e); await refresh().catch(() => {}) }
    finally { busy = false }
  }
  async function startName(action) {
    menu = null; proposedName = action === 'rename' ? selected.name : ''
    naming = {action,paths:action === 'rename' ? picked() : [],target:destination()}
    error = ''; await tick(); nameInput?.focus(); nameInput?.select()
  }
  async function copyPaths(relative) {
    menu = null
    const prefix = listing.path.replace(/[\\/]$/, '')
    try { await navigator.clipboard.writeText(picked().map(path => relative ? path.slice(prefix.length+1) : path).join('\n')); status = 'Path copied.' }
    catch(e) { error = String(e) }
  }
  function copy() { clipboard = picked(); menu = null; status = `${clipboard.length} items copied. Choose a folder and paste.` }
  async function context(event, entry) {
    event.preventDefault(); event.stopPropagation()
    if (entry.path === listing.path) selection = new Set()
    else if (!selection.has(entry.path)) selection = new Set([entry.path])
    selected = entry; focused = entry.path
    const bounds = event.currentTarget.getBoundingClientRect()
    menu = {x:Math.max(8,Math.min(event.clientX || bounds.left,window.innerWidth-250)),y:Math.max(8,Math.min(event.clientY || bounds.bottom,window.innerHeight-450))}
    await tick(); menuElement?.querySelector('button:not(:disabled)')?.focus()
  }
  function menuKeys(event) {
    if (event.key === 'Escape') { event.preventDefault(); menu = null; void focus(focused); return }
    if (!['ArrowUp','ArrowDown','Home','End','Tab'].includes(event.key)) return
    event.preventDefault()
    const buttons = [...menuElement.querySelectorAll('button:not(:disabled)')]
    const index = buttons.indexOf(document.activeElement)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length-1 : (index + (event.key === 'ArrowUp' || event.shiftKey ? -1 : 1) + buttons.length)%buttons.length
    buttons[next]?.focus()
  }
  function outside(event) { if (menu && !menuElement?.contains(event.target)) menu = null }

  let filter = $state('')
  let version = 0
  let children = $state({})
  let expanded = $state(new Set())
  let reading = $state(new Set())
  let folderErrors = $state({})
  let focused = $state(null)
  let tree = $state()
  const allEntries = $derived(fileTreeRows(listing?.entries ?? [],children,expanded,filter))
  const entries = $derived(allEntries.filter(entry => showHidden || !entry.path.slice((listing?.path.length || 0) + 1).split(/[\\/]/).some(part => part.startsWith('.'))))
  const tabStop = $derived(entries.some(e => e.path === focused) ? focused : entries[0]?.path)
  async function toggle(entry) {
    if (expanded.has(entry.path)) { expanded = new Set([...expanded].filter(path => path !== entry.path)); return }
    expanded = new Set(expanded).add(entry.path)
    if (children[entry.path] || reading.has(entry.path)) return
    const request = version
    reading = new Set(reading).add(entry.path)
    folderErrors = {...folderErrors,[entry.path]:''}
    try {
      const result = await tauri.invoke('workspace_list',{path:entry.path})
      if (request === version) children = {...children,[entry.path]:result.entries}
    } catch(e) { if (request === version) folderErrors = {...folderErrors,[entry.path]:String(e)} }
    finally { if (request === version) reading = new Set([...reading].filter(path => path !== entry.path)) }
  }
  async function focus(path) {
    focused = path
    await tick()
    ;[...tree.querySelectorAll('[role="treeitem"]')].find(e => e.dataset.path === path)?.focus()
  }
  function keys(event,entry) {
    const mod = event.metaKey || event.ctrlKey
    if (mod && event.key.toLowerCase() === 'a') { event.preventDefault(); selection = new Set(entries.map(e => e.path)); return }
    if (mod && event.key.toLowerCase() === 'c') { event.preventDefault(); copy(); return }
    if (mod && event.key.toLowerCase() === 'v') { event.preventDefault(); if (clipboard.length) void run('paste',clipboard); return }
    if (event.key === 'Delete' || event.key === 'Backspace') { event.preventDefault(); if (!selection.size) selection = new Set([entry.path]); deleting = picked(); return }
    if (event.key === 'F2') { event.preventDefault(); selected = entry; selection = new Set([entry.path]); void startName('rename'); return }
    if (event.key === ' ' && mod) { event.preventDefault(); void select(entry,event); return }
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) { void context(event,entry); return }
    const index = entries.findIndex(e => e.path === entry.path)
    let next
    if (event.key === 'ArrowDown') next = entries[Math.min(entries.length-1,index+1)]?.path
    else if (event.key === 'ArrowUp') next = entries[Math.max(0,index-1)]?.path
    else if (event.key === 'Home') next = entries[0]?.path
    else if (event.key === 'End') next = entries.at(-1)?.path
    else if (event.key === 'ArrowRight' && entry.directory) {
      if (!expanded.has(entry.path)) void toggle(entry)
      else if (entries[index+1]?.parent === entry.path) next = entries[index+1].path
    } else if (event.key === 'ArrowLeft') {
      if (entry.directory && expanded.has(entry.path)) void toggle(entry)
      else next = entry.parent
    } else return
    event.preventDefault()
    if (next) {
      if (event.shiftKey) selection = new Set([...selection,entry.path,next])
      else if (!mod) selection = new Set([next])
      selected = entries.find(item => item.path === next) ?? selected
      void focus(next)
    }
  }
  async function load(path) {
    const request = ++version
    loading = true; error = ''; selected = null; selection = new Set(); menu = null
    children = {}; expanded = new Set(); reading = new Set(); folderErrors = {}; focused = null
    try {
      const result = await tauri.invoke('workspace_list', { path })
      if (request !== version) return
      listing = result; filter = ''; onfolder?.(result.path)
    } catch (e) { if (request === version) error = String(e) }
    finally { if (request === version) loading = false }
  }
  async function choose() {
    try { const path = await open({ directory: true, multiple: false }); if (typeof path === 'string') await load(path) }
    catch (e) { error = String(e) }
  }
  async function select(entry, event = {}) {
    const previous = focused
    focused = entry.path; selected = entry
    if (event.shiftKey && previous) {
      const a = entries.findIndex(e => e.path === previous), b = entries.findIndex(e => e.path === entry.path)
      selection = new Set([...selection,...entries.slice(Math.min(a,b),Math.max(a,b)+1).map(e => e.path)]); return
    }
    if (event.metaKey || event.ctrlKey) {
      const next = new Set(selection); if (next.has(entry.path)) next.delete(entry.path); else next.add(entry.path); selection = next; return
    }
    selection = new Set([entry.path])
    if (entry.directory) return toggle(entry)
    onfile?.(entry)
  }
  onMount(() => {
    void tauri.invoke('workspace_folders', workspaceContext || { threadId, projectId }).then(result => {
      folders = result
      let saved
      try { saved = JSON.parse(localStorage.getItem(storageKey) || 'null') } catch { /* Ignore invalid saved state. */ }
      const path = initialPath || saved?.path || result.at(-1)?.path
      if (path) return load(path).then(async () => {
        if (!listing && result.at(-1)?.path !== path) await load(result.at(-1).path)
        for (const entry of saved?.expanded || []) {
          if (!listing || !(entry.startsWith(listing.path + '/') || entry.startsWith(listing.path + '\\'))) continue
          try { children = {...children, [entry]:(await tauri.invoke('workspace_list',{path:entry})).entries}; expanded = new Set([...expanded,entry]) } catch { /* A removed folder stays closed. */ }
        }
      })
    }).catch(e => { error = String(e) })
    return () => { if (listing) { try { localStorage.setItem(storageKey,JSON.stringify({path:listing.path,expanded:[...expanded]})) } catch { /* Storage may be unavailable. */ } }; version += 1 }
  })
</script>
<svelte:window onpointerdown={outside} />
<section class="files-workspace" class:hidden aria-label="Files">
  <header>
    <button type="button" aria-label="Parent folder" disabled={busy || !listing?.parent} onclick={() => load(listing.parent)}><LucideIcon name="arrow-left" /></button>
    <select disabled={busy} aria-label="Workspace folder" value={listing?.path ?? ''} onchange={e => load(e.target.value)}>
      <option value="" disabled>Choose a workspace</option>
      {#each folders as folder, i (i)}<option value={folder.path}>{folder.label}</option>{/each}
      {#if listing && !folders.some(f => f.path === listing.path)}<option value={listing.path}>{listing.path}</option>{/if}
    </select>
    <button type="button" disabled={busy} onclick={choose}>Choose folder…</button>
    <button type="button" aria-label="Refresh files" disabled={busy || !listing} onclick={() => refresh().catch(e => error = String(e))}><LucideIcon name="refresh-cw" /></button>
  </header>
  {#if listing}<p class="path"><OverflowText text={listing.path} /></p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  <div class="files-content">
    <div class="file-list" use:panelScroll role="group" aria-label="File list" oncontextmenu={event => { if (listing) void context(event,{path:listing.path,directory:true,name:''}) }}>
      <div class="filter-row"><input aria-label="Filter files" placeholder="Filter files" bind:value={filter} /><button type="button" aria-label="New file" disabled={!listing || busy} onclick={() => startName('new-file')}><LucideIcon name="file-plus" /></button><button type="button" aria-label="New folder" disabled={!listing || busy} onclick={() => startName('new-folder')}><LucideIcon name="folder-plus" /></button></div>
      <label class="hidden-toggle"><input type="checkbox" bind:checked={showHidden} />Show hidden files</label>
      {#if status}<p role="status">{status}</p>{/if}
      {#if selection.size > 1}<p>{selection.size} selected</p>{/if}
      {#if loading}<p role="status">Reading folder…</p>
      {:else if !entries.length}<p>{filter ? "No matching files." : "No files here yet. Create a file or choose a folder."}</p>
      {:else}<div role="tree" aria-label="Files and folders" aria-multiselectable="true" bind:this={tree}>{#each entries as entry (entry.path)}
        <button class="tree-row" disabled={busy} type="button" role="treeitem" data-path={entry.path} aria-label={entry.name} aria-level={entry.depth+1} aria-expanded={entry.directory ? expanded.has(entry.path) : undefined} aria-selected={selection.has(entry.path)} tabindex={tabStop === entry.path ? 0 : -1} style:padding-left={`${6+entry.depth*18}px`} oncontextmenu={event => context(event,entry)} onclick={event => select(entry,event)} onkeydown={event => keys(event,entry)}>
          <span class="disclosure">{#if entry.directory}<LucideIcon name={expanded.has(entry.path) ? 'chevron-down' : 'chevron-right'} size={14} />{/if}</span><FileIcon name={entry.name} directory={entry.directory} /><span class="file-name">{entry.name}</span>
        </button>
        {#if expanded.has(entry.path) && reading.has(entry.path)}<p class="folder-status" role="status">Reading {entry.name}…</p>{/if}
        {#if expanded.has(entry.path) && folderErrors[entry.path]}<p class="folder-status" role="alert">{folderErrors[entry.path]}</p>{/if}
      {/each}</div>{/if}
    </div>
  </div>
</section>
{#if menu && !hidden}
  <div class="file-menu" bind:this={menuElement} use:floatingMenu role="menu" tabindex="-1" aria-label="File actions" style:left={`${menu.x}px`} style:top={`${menu.y}px`} onkeydown={menuKeys}>
    <button role="menuitem" disabled={busy} onclick={() => startName('new-file')}>New File…</button>
    <button role="menuitem" disabled={busy} onclick={() => startName('new-folder')}>New Folder…</button>
    <hr />
    <button role="menuitem" disabled={busy || selection.size !== 1} onclick={() => startName('rename')}>Rename…</button>
    <button role="menuitem" disabled={busy || !selection.size} onclick={() => run('duplicate')}>Duplicate</button>
    <hr />
    <button role="menuitem" onclick={() => { menu = null; tauri.invoke('workspace_reveal',{path:selected.path}).catch(e => error = String(e)) }}>{revealLabel}</button>
    <button role="menuitem" disabled={!selection.size} onclick={copy}>Copy</button>
    <button role="menuitem" disabled={!clipboard.length || busy} onclick={() => run('paste',clipboard)}>Paste</button>
    <button role="menuitem" disabled={!selection.size} onclick={() => copyPaths(false)}>Copy Path</button>
    <button role="menuitem" disabled={!selection.size} onclick={() => copyPaths(true)}>Copy Relative Path</button>
    <hr />
    <button role="menuitem" class="delete" disabled={busy || !selection.size} onclick={() => { deleting = picked(); menu = null }}>Delete{selection.size > 1 ? ` ${selection.size} items` : ''}…</button>
  </div>
{/if}
{#if deleting}<ConfirmDialog title={`Delete ${deleting.length} ${deleting.length === 1 ? 'item' : 'items'}?`} cancelLabel="Cancel" confirmLabel="Move to Trash" onDecision={yes => yes ? run('trash',deleting) : deleting = null}><p>The selected items will move to the system Trash.</p>{#if error}<p role="alert">{error}</p>{/if}</ConfirmDialog>{/if}
{#if naming}
  <div class="name-backdrop" use:dialogDismiss={{onclose: () => naming = null, disabled: busy}}><form role="dialog" aria-modal="true" aria-label={naming.action === 'rename' ? 'Rename item' : naming.action === 'new-file' ? 'New file' : 'New folder'} onsubmit={e => { e.preventDefault(); void run(naming.action,naming.paths,proposedName,naming.target) }} onkeydown={e => { if (e.key === 'Escape' && !busy) naming = null; if (e.key === 'Tab') { const controls = [...e.currentTarget.querySelectorAll('input,button:not(:disabled)')]; const index = controls.indexOf(document.activeElement); e.preventDefault(); controls[(index+(e.shiftKey?-1:1)+controls.length)%controls.length]?.focus() } }}>
    <header class="name-head"><label for="workspace-item-name">{naming.action === 'rename' ? 'Rename item' : naming.action === 'new-file' ? 'New file' : 'New folder'}</label><PopupClose label="Close file name" disabled={busy} onclick={() => naming = null} /></header>
    <input id="workspace-item-name" bind:this={nameInput} bind:value={proposedName} disabled={busy} autocomplete="off" />
    {#if error}<p role="alert">{error}</p>{/if}
    <div class="name-actions"><button type="submit" disabled={busy || !proposedName.trim()}>{naming.action === 'rename' ? 'Rename' : 'Create'}</button></div>
  </form></div>
{/if}
<style>
  .name-head { display:flex; align-items:start; justify-content:space-between; gap:12px; }
  .filter-row { display:flex; align-items:center; gap:4px; margin-bottom:8px; } .filter-row input { min-width:0; flex:1; margin:0; } .filter-row button { padding:6px; }
  .file-menu { position:fixed; z-index:12; width:240px; max-height:calc(100vh - 16px); overflow:auto; background:var(--surface); padding:5px; border:1px solid var(--border); border-radius:var(--radius-panel); box-shadow:var(--shadow-overlay); }
  .file-menu button { border:0; width:100%; min-height:32px; font-family:var(--font-human); } .file-menu .delete { color:light-dark(#b42318,#ff7676); } hr { border:0; border-top:1px solid var(--border); margin:5px; }
  .name-backdrop { position:fixed; inset:0; z-index:13; display:grid; place-items:center; background:var(--overlay-backdrop); } form { width:min(400px,90vw); padding:20px; border:1px solid var(--border); border-radius:var(--radius-panel); background:var(--surface); } label { display:block; margin-bottom:12px; } .name-actions { display:flex; gap:8px; justify-content:flex-end; }
  .files-workspace { flex:1; display:flex; flex-direction:column; min-height:0; min-width:0; background:var(--surface); overflow:hidden; }
  header { display:flex; align-items:center; gap:6px; padding:8px; border-bottom:1px solid var(--border); }
  button,input,select { border:1px solid var(--border); border-radius:var(--radius-control); background:var(--surface); color:var(--ink); padding:5px 8px; font:var(--text-12) var(--font-mono); }
  button { display:flex; align-items:center; gap:8px; cursor:pointer; } button:disabled { opacity:.5; } button:hover:not(:disabled),button[aria-selected="true"] { background:var(--faint); }
  select { flex:1; min-width:0; } p { margin:12px; color:var(--muted); font:var(--text-12) var(--font-mono); } .path { min-width:0; }
  .files-content { flex:1; min-height:0; display:grid; grid-template-columns:minmax(0,1fr); }
  .file-list { min-height:0; overflow:auto; padding:8px; } input { width:100%; margin-bottom:8px; }
  .hidden-toggle { display:flex; align-items:center; gap:6px; margin:4px 0 8px; color:var(--muted); font:var(--text-12) var(--font-human); }
  .hidden-toggle input { width:14px; height:14px; margin:0; padding:0; flex:none; }
  .hidden { display:none; }
  .tree-row { width:100%; text-align:left; border:0; min-height:28px; gap:6px; }
  .tree-row[aria-selected="true"] { background:var(--faint); }
  .file-name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .disclosure { width:14px; flex:none; display:flex; }
  .folder-status { margin:4px 12px; }
</style>

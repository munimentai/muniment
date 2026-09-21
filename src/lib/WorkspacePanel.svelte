<script>
  import { tick, untrack, onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { confirm } from '@tauri-apps/plugin-dialog'
  import LucideIcon from './LucideIcon.svelte'
  import OverflowText from './OverflowText.svelte'
  import { folderLabel, browserLabel } from './workspace-labels.js'
  import { overflowFade } from './overflow-fade.js'
  import BrowserWorkspace from './BrowserWorkspace.svelte'
  import TerminalWorkspace from './TerminalWorkspace.svelte'
  import FilesWorkspace from '../files/FilesWorkspace.svelte'
  import WorkspaceFile from '../files/WorkspaceFile.svelte'
  import FileIcon from '../files/FileIcon.svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  let { context = null, tauri, selected = null, onselect, threadId = null, projectId = null, suspended = false, requestedFile = null, requestedArtifact = null, navigation = null, onnavigationhandled, onfolder } = $props()
  let tabs = $state([])
  let browserUrl = $state('')
  let folderPaths = $state({})
  let terminalPaths = $state({})
  let tabList
  function tabName(tab) {
    if (tab.id === 'browser' && browserUrl) return browserLabel(browserUrl)
    if (tab.id === 'files') return folderLabel(folderPaths[contextKey]) || tab.name
    if (tab.id === 'terminal') return folderLabel(terminalPaths[contextKey]) || tab.name
    return tab.name
  }
  let dirtyFiles = $state(new Set())
  let closing = $state(null)
  function markDirty(id, dirty) { const next = new Set(dirtyFiles); if (dirty) next.add(id); else next.delete(id); dirtyFiles = next }
  const contextKey = $derived(JSON.stringify(context ? Object.fromEntries(Object.entries(context).filter(([key]) => key !== 'threadTitle')) : {threadId, projectId}))
  let terminalSessions = $state([])
  $effect(() => {
    const key = contextKey
    if (selected === 'terminal') untrack(() => {
      if (!terminalSessions.some(session => session.key === key)) terminalSessions = [...terminalSessions, {key, context: context ? {...context} : {threadId, projectId}}]
    })
  })
  let maximized = $state(false)
  const tools = { browser:{name:'Browser',icon:'globe'},files:{name:'Files',icon:'folder'},terminal:{name:'Terminal',icon:'square-terminal'},artifacts:{name:'Artifacts',icon:'file'} }
  onMount(() => {
    let off, gone = false
    try {
      const window = getCurrentWindow()
      window.onCloseRequested(async event => {
        if (!dirtyFiles.size) return
        event.preventDefault()
        if (await confirm('Your open files have unsaved changes.', {title:'Discard unsaved changes?', kind:'warning', okLabel:'Discard', cancelLabel:'Cancel'})) await window.destroy()
      }).then(unlisten => { if (gone) unlisten(); else off = unlisten }).catch(() => {})
    } catch { /* Browser previews have no native window. */ }
    return () => { gone = true; off?.() }
  })
  function canChange(paths) {
    return !tabs.some(tab => dirtyFiles.has(tab.id) && paths.some(path => tab.file?.path === path || tab.file?.path.startsWith(path + '/') || tab.file?.path.startsWith(path + '\\')))
  }
  function filesChanged({action, paths}) {
    if (!['rename','trash'].includes(action)) return
    for (const tab of [...tabs]) if (paths.some(path => tab.file?.path === path || tab.file?.path.startsWith(path + '/') || tab.file?.path.startsWith(path + '\\'))) close(tab.id)
  }
  function openFile(file) {
    const id = `file:${file.path}`
    if (!tabs.some(t => t.id === id)) tabs = [...tabs,{id,name:file.name,file}]
    onselect(id)
  }
  function close(id, discard = false) {
    if (dirtyFiles.has(id) && !discard) { closing = id; return }
    if (id === 'terminal') terminalSessions = []
    markDirty(id, false)
    const index = tabs.findIndex(t => t.id === id)
    tabs = tabs.filter(t => t.id !== id)
    if (selected === id) onselect(tabs[Math.min(index,tabs.length-1)]?.id ?? null)
  }
  async function keys(e) {
    if (!['ArrowLeft','ArrowRight','Home','End','Delete'].includes(e.key)) return
    e.preventDefault()
    if (e.key === 'Delete') { close(selected); return }
    const index = tabs.findIndex(t => t.id === selected)
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length-1 : (index+(e.key === 'ArrowRight'?1:-1)+tabs.length)%tabs.length
    onselect(tabs[next].id)
    const list = e.currentTarget
    await tick()
    list?.querySelector('[aria-selected="true"]')?.focus()
  }
  $effect(() => { if (selected && tools[selected] && !tabs.some(t => t.id === selected)) tabs = [...tabs,{id:selected,...tools[selected]}] })
  $effect(() => {
    selected; tabs.length
    void tick().then(() => tabList?.querySelector('[aria-selected="true"]')?.closest('.tab')?.scrollIntoView?.({block: 'nearest', inline: 'nearest'}))
  })
  $effect(() => { const file = requestedFile; if (file) untrack(() => openFile(file)) })
</script>
<aside data-panel="workspace" class="workspace-panel" class:hidden={!selected} class:maximized aria-label="Workspace panel">
  <header data-panel-header class="tabs-header">
    <div class="tabs fade-viewport" bind:this={tabList} use:overflowFade role="tablist" tabindex="-1" aria-label="Open workspace tabs" onkeydown={keys}>
      {#each tabs as tab (tab.id)}
        <div class="tab" class:active={selected === tab.id}>
          <button type="button" role="tab" aria-selected={selected === tab.id} tabindex={selected === tab.id ? 0 : -1} onclick={() => onselect(tab.id)}>
            {#if tab.file}<FileIcon name={tab.name} />{:else}<LucideIcon name={tab.icon} />{/if}<OverflowText text={tabName(tab)} side={tab.id === 'browser' ? 'right' : 'left'} scroll={false} />{#if dirtyFiles.has(tab.id)}<span class="dirty" aria-label="Unsaved changes">•</span>{/if}
          </button>
          <button class="tab-close" type="button" aria-label={`Close ${tab.name} tab`} onclick={() => close(tab.id)}><LucideIcon name="x" size={12} /></button>
        </div>
      {/each}
    </div>
    <button data-panel-control class="panel-control" type="button" aria-label={maximized ? 'Restore workspace panel' : 'Expand workspace panel'} onclick={() => maximized = !maximized}><LucideIcon name={maximized ? 'minimize-2' : 'maximize-2'} /></button>
    <button data-panel-control class="panel-control" type="button" aria-label="Hide workspace panel" onclick={() => onselect(null)}><LucideIcon name="panel-right-close" /></button>
  </header>
  {#if selected === 'browser' || selected === 'artifacts'}{#key selected}
    <BrowserWorkspace onurl={url => browserUrl = url} {tauri} {requestedArtifact} {navigation} {onnavigationhandled} artifacts={selected === 'artifacts'} suspended={suspended || !!closing} />
  {/key}{/if}
  {#if tabs.some(t => t.id === 'files')}{#key contextKey}<FilesWorkspace {context} onwillchange={canChange} onchanged={filesChanged} hidden={selected !== 'files'} {tauri} {threadId} {projectId} initialPath="" onfolder={path => { folderPaths = {...folderPaths, [contextKey]: path}; onfolder?.(path) }} onfile={openFile} />{/key}{/if}
  {#each terminalSessions as session (session.key)}<TerminalWorkspace oncwd={path => terminalPaths = {...terminalPaths, [session.key]: path}} {tauri} context={session.context} hidden={selected !== 'terminal' || session.key !== contextKey} />{/each}
  {#each tabs.filter(t => t.file) as tab (tab.id)}
    <div class="file-view" class:hidden={selected !== tab.id}><WorkspaceFile {tauri} file={tab.file} {threadId} ondirty={dirty => markDirty(tab.id, dirty)} /></div>
  {/each}
</aside>
{#if closing}<ConfirmDialog title="Discard unsaved changes?" cancelLabel="Cancel" confirmLabel="Discard" onDecision={yes => { if (yes) close(closing, true); closing = null }}><p>Your edits to {tabs.find(t => t.id === closing)?.name} have not been saved.</p></ConfirmDialog>{/if}
<style>
  .file-view { display:flex; flex:1; min-height:0; min-width:0; } .file-view.hidden { display:none; }
  .workspace-panel { grid-area:rail; display:flex; flex-direction:column;   overflow:hidden;    }
  .workspace-panel.hidden { display:none; }
  .workspace-panel.maximized:not(.hidden) { grid-column:2 / -1; grid-row:2; z-index:3; }
  .tabs-header { display:flex; align-items:center; gap:3px; padding:var(--panel-control-inset); min-height:var(--panel-control-size); border-bottom:1px solid var(--border); }
  .tabs { display:flex; flex:1; min-width:0; overflow-x:auto; overflow-y:hidden; gap:3px; }
  .tab { display:flex; align-items:center; flex:0 0 160px; width:160px; min-width:0; border-radius:var(--radius-control); }
  .tab.active { background:var(--faint); }
  button { display:flex; align-items:center; gap:6px; min-width:0; padding:5px 6px; border:0; border-radius:var(--radius-control); background:transparent; color:var(--muted); font:var(--text-12) var(--font-human); cursor:pointer; }
  .active button { color:var(--ink); } button:hover { background:var(--faint); }
  .tab button[role="tab"] { flex:1; overflow:hidden; }
  .tab :global(svg), .dirty { flex:none; }
  .tab-close { flex:none; padding:4px; } .panel-control { flex:none; width:28px; height:28px; justify-content:center; }
</style>

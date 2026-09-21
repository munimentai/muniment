<script>
  import { panelScroll } from './panel-scroll.js'
  import CatalogActions from './CatalogActions.svelte'
  import LucideIcon from './LucideIcon.svelte'
  let {items=[],onopen,oncreate,onclose,isArchived=()=>false,onaction=()=>{}}=$props()
  let archived = $state(false)
</script>
<section data-panel="artifacts" use:panelScroll class="panel" aria-label="Artifacts"><header data-panel-header><h2>Artifacts</h2><button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</button><button onclick={oncreate}>New artifact</button><button aria-label="Close artifacts" onclick={onclose}><LucideIcon name="x" /></button></header>
<div class="catalog">{#each items.filter(item=>isArchived('artifact',item)===archived) as item (item.threadId || item.id)}<div class="card-wrap"><CatalogActions name={item.name || item.goal} {archived} onaction={(action,name)=>onaction('artifact',item,action,name)}/><button class="card" onclick={()=>onopen(item)}><LucideIcon name="file-code" size={32}/><strong>{item.name || item.goal}</strong><span>{item.output || 'Open the artifact chat'}</span></button></div>{/each}</div></section>
<style>
.card-wrap {position:relative;display:flex} .card-wrap :global(.catalog-actions) {position:absolute;right:8px;top:8px} .card {width:100%;padding-top:32px!important}
.panel {grid-area:thread;overflow:auto;padding:0;} header{display:flex;align-items:center;gap:12px} h2{flex:1;font-size:var(--text-22)} button{background:var(--surface);color:var(--ink);border:1px solid var(--border);border-radius:var(--radius-control);padding:8px 12px;cursor:pointer;font:var(--text-13) var(--font-human)} .catalog{padding:0 12px 12px;display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:12px} .card{display:flex;flex-direction:column;align-items:flex-start;gap:12px;text-align:left;padding:20px;overflow-wrap:anywhere}.card:hover{background:var(--faint)}span{color:var(--muted)}
</style>

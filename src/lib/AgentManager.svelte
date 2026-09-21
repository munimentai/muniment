<script>
  import { panelScroll } from './panel-scroll.js'
  import {onMount} from 'svelte'
  import CatalogActions from './CatalogActions.svelte'
  import AgentAvatar from './AgentAvatar.svelte'
  import LucideIcon from './LucideIcon.svelte'
  let {tauri, projects=[], onclose, onselect, oncreate, onchange=()=>{}, pending=[], onopen, agents=null, isArchived=()=>false, onaction=()=>{}}=$props()
  let archived = $state(false)
  let listing=$state({agents:[],state:{runs:{},threads:{}}}), error=$state(''), loading=$state(true)
  onMount(()=>{let alive=true;tauri.invoke('agent_list').then(value=>{if(alive){listing=value;onchange(value)}}).catch(e=>error=String(e)).finally(()=>loading=false);return()=>{alive=false}})
</script>
<section data-panel="agents" use:panelScroll class="panel" aria-label="Agents">
  <header data-panel-header><h2>Agents</h2><button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</button><button onclick={()=>oncreate?.()}>New agent</button><button aria-label="Close agents" onclick={onclose}><LucideIcon name="x" /></button></header>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if loading}<p role="status">Loading agents…</p>{/if}
  <div class="catalog">
    {#each (agents ?? listing.agents).filter(agent=>isArchived('agent',agent)===archived) as agent (agent.id)}<div class="card-wrap"><CatalogActions name={agent.name} {archived} onaction={(action,name)=>onaction('agent',agent,action,name)}/><button class="card" onclick={()=>onselect(agent)}><AgentAvatar {agent} size={48} /><strong>{agent.name}</strong><span>{agent.label || agent.instructions}</span><small>{projects.find(([id])=>id===agent.projectId)?.[1] || 'No project'}</small></button></div>{/each}
    {#each pending.filter(plan=>isArchived('creation',plan)===archived) as plan (plan.threadId)}<div class="card-wrap"><CatalogActions name={plan.goal} {archived} onaction={(action,name)=>onaction('creation',plan,action,name)}/><button class="card" onclick={()=>onopen(plan)}><LucideIcon name="bot" size={32}/><strong>{plan.goal}</strong><span>{plan.output}</span><small>Continue creation</small></button></div>{/each}
  </div>
</section>
<style>
  .panel {grid-area:thread;overflow:auto;padding:0;}
  header {display:flex;align-items:center;gap:12px} h2 {flex:1;font-size:var(--text-22)} button {background:var(--surface);color:var(--ink);border:1px solid var(--border);border-radius:var(--radius-control);padding:8px 12px;cursor:pointer;font:var(--text-13) var(--font-human)}
  .card-wrap {position:relative;display:flex} .card-wrap :global(.catalog-actions) {position:absolute;right:8px;top:8px} .card {width:100%;padding-top:32px!important}
  .catalog {padding:0 12px 12px;display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:12px} .card {display:flex;flex-direction:column;align-items:flex-start;gap:10px;text-align:left;padding:20px;overflow-wrap:anywhere} .card:hover {background:var(--faint)} span,small,p {color:var(--muted)} span {display:-webkit-box;-webkit-line-clamp:3;-webkit-box-orient:vertical;overflow:hidden}
</style>

<script>
  import { creationTitle } from './thread-title.js'
  import PanelPage from './ui/PanelPage.svelte'
  import PageHead from './ui/PageHead.svelte'
  import CardGrid from './ui/CardGrid.svelte'
  import CatalogCard from './ui/CatalogCard.svelte'
  import Button from './ui/Button.svelte'
  import {onMount} from 'svelte'
  import CatalogActions from './CatalogActions.svelte'
  import AgentAvatar from './AgentAvatar.svelte'
  import LucideIcon from './LucideIcon.svelte'
  let {tauri, projects=[], onclose, onselect, oncreate, onchange=()=>{}, pending=[], threadSummaries=[], onopen, agents=null, isArchived=()=>false, onaction=()=>{}}=$props()
  let archived = $state(false)
  let listing=$state({agents:[],state:{runs:{},threads:{}}}), error=$state(''), loading=$state(true)
  onMount(()=>{let alive=true;tauri.invoke('agent_list').then(value=>{if(alive){listing=value;onchange(value)}}).catch(e=>error=String(e)).finally(()=>loading=false);return()=>{alive=false}})
</script>
<section data-panel="agents" class="panel" aria-label="Agents">
  <h2 class="visually-hidden">Agents</h2>
  <PanelPage>
    <PageHead intro="Each agent keeps its own instructions and memory, and works in its own chat.">
      {#snippet actions()}
        <Button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</Button>
        <Button onclick={()=>oncreate?.()}>New agent</Button>
        <button type="button" aria-label="Close agents" onclick={onclose}><LucideIcon name="x" /></button>
      {/snippet}
    </PageHead>
    {#if error}<p role="alert">{error}</p>{/if}
    {#if loading}<p role="status">Loading agents…</p>{/if}
    <CardGrid>
      {#each (agents ?? listing.agents).filter(agent=>isArchived('agent',agent)===archived) as agent (agent.id)}
        <CatalogCard onclick={()=>onselect(agent)}>
          {#snippet menu()}<CatalogActions name={agent.name} {archived} onaction={(action,name)=>onaction('agent',agent,action,name)}/>{/snippet}
          <AgentAvatar {agent} size={48} /><strong>{agent.name}</strong><span>{agent.label || agent.instructions}</span><small>{projects.find(([id])=>id===agent.projectId)?.[1] || 'No project'}</small>
        </CatalogCard>
      {/each}
      {#each pending.filter(plan=>isArchived('creation',plan)===archived) as plan (plan.threadId)}
        <CatalogCard onclick={()=>onopen(plan)}>
          {#snippet menu()}<CatalogActions name={creationTitle(plan, threadSummaries)} {archived} onaction={(action,name)=>onaction('creation',plan,action,name)}/>{/snippet}
          <LucideIcon name="bot" size={32}/><strong>{creationTitle(plan, threadSummaries)}</strong><span>{plan.output}</span><small>Continue creation</small>
        </CatalogCard>
      {/each}
    </CardGrid>
    {#if !loading && !error && !(agents ?? listing.agents).some(agent=>isArchived('agent',agent)===archived) && !pending.some(plan=>isArchived('creation',plan)===archived)}<p>{archived ? 'No archived agents.' : 'No agents yet. New agent starts one in a chat.'}</p>{/if}
  </PanelPage>
</section>
<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
</style>

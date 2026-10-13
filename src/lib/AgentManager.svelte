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
  import StarterRow from './StarterRow.svelte'
  import { creationStarters } from './creation-prompts.js'
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
    {#if !archived && !loading}
      {@const some = (agents ?? listing.agents).some(agent=>!isArchived('agent',agent)) || pending.some(plan=>!isArchived('creation',plan))}
      <StarterRow heading="Give an agent a job" kind="agent" items={creationStarters.agent} compact={some} oncreate={(item)=>oncreate?.({goal:item.goal,output:item.output})} />
      {#if !some}
        <ul class="benefits">
          <li><LucideIcon name="brain" />Keeps its own instructions and memory</li>
          <li><LucideIcon name="clock" />Runs on a schedule you set</li>
          <li><LucideIcon name="messages-square" />Works in a chat of its own</li>
        </ul>
      {/if}
    {/if}
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
    {#if !loading && !error && !(agents ?? listing.agents).some(agent=>isArchived('agent',agent)===archived) && !pending.some(plan=>isArchived('creation',plan)===archived)}{#if archived}<p>No archived agents.</p>{/if}{/if}
  </PanelPage>
</section>
<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .benefits { display: flex; flex-wrap: wrap; gap: 8px 24px; margin: 0; padding: 0; list-style: none; color: var(--muted); font-size: var(--text-13); }
  .benefits li { display: inline-flex; align-items: center; gap: 8px; }
</style>

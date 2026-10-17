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
  import MachineMark from '../agents/MachineMark.svelte'
  import AgentImport from '../agents/AgentImport.svelte'
  import { agentMachine, agentRuntimes, emptyListing, listAgents } from '../agents/agent-runtime.js'
  import { triggerSummary } from '../agents/routine-labels.js'
  import { pairedRuntimes } from '../runtimes/runtimes.js'
  let {tauri, projects=[], onselect, oncreate, onchange=()=>{}, onimported=()=>{}, pending=[], threadSummaries=[], onopen, agents=null, runtimeList=null, isArchived=()=>false, onaction=()=>{}}=$props()
  let archived = $state(false)
  let importOpen = $state(false)
  // The machines an import can land on: this computer and each that answers.
  const importRuntimes = $derived(agentRuntimes(runtimeList).map(runtime => ({ id: runtime.id, name: runtime.local ? 'This computer' : runtime.name })))
  const machineOf = (agent) => agent.primaryMachine ?? agentMachine(runtimeList, agent)
  const showMachines = $derived(pairedRuntimes(runtimeList).length > 0 || (agents ?? []).some(agent => agent.replica || (agent.primaryMachine && agent.primaryMachine.kind !== 'local')))
  let listing=$state(emptyListing()), error=$state(''), loading=$state(true)
  onMount(()=>{let alive=true;listAgents(tauri, runtimeList).then(value=>{if(alive){listing=value;onchange(value)}}).catch(e=>error=String(e?.message ?? e)).finally(()=>loading=false);return()=>{alive=false}})
  // The card's note: the project and the first routine that is on.
  const cardNote = (agent) => [projects.find(([id])=>id===agent.projectId)?.[1], (agent.routines ?? []).filter(routine=>routine.enabled).map(routine=>triggerSummary(routine.trigger))[0]].filter(Boolean).join(' · ') || 'Runs when you ask'
</script>
<section data-panel="agents" class="panel" aria-label="Agents">
  <h2 class="visually-hidden">Agents</h2>
  <PanelPage>
    <PageHead intro="Each agent keeps its own instructions and memory, and works in its own chat.">
      {#snippet actions()}
        {#if archived || (agents ?? listing.agents).some(agent=>isArchived('agent',agent)) || pending.some(plan=>isArchived('creation',plan))}<Button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</Button>{/if}
        <Button icon="download" onclick={()=>importOpen=true}>Import agent</Button>
        <Button icon="plus" onclick={()=>oncreate?.()}>New agent</Button>
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
          <li><LucideIcon name="clock" />Runs on a schedule, a webhook or a tool check</li>
          <li><LucideIcon name="messages-square" />Works in a chat of its own</li>
        </ul>
      {/if}
    {/if}
    <CardGrid>
      {#each (agents ?? listing.agents).filter(agent=>isArchived('agent',agent)===archived) as agent (`${agent.runtimeId ?? 'local'}:${agent.id}`)}
        <CatalogCard onclick={()=>onselect(agent)}>
          {#snippet menu()}{#if !agent.replica}<CatalogActions name={agent.name} {archived} onaction={(action,name)=>onaction('agent',agent,action,name)}/>{/if}{/snippet}
          <AgentAvatar {agent} activity={agent.activity} size={48} /><strong>{agent.name}</strong><span>{agent.label || agent.instructions}</span><small>{agent.replica ? 'Synced copy from another machine' : cardNote(agent)}</small>{#if showMachines}<div class="machine"><MachineMark machine={machineOf(agent)} />{#if agent.online === false}<span class="offline">Offline</span>{/if}{#if agent.replica}<span class="synced"><LucideIcon name="refresh-cw" size={12} /><span class="visually-hidden">Synced copy</span></span>{/if}</div>{/if}
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
{#if importOpen}<AgentImport {tauri} runtimes={importRuntimes} onclose={()=>importOpen=false} onimported={(agent)=>onimported(agent)} />{/if}
<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .benefits { display: flex; flex-wrap: wrap; gap: 8px 24px; margin: 0; padding: 0; list-style: none; color: var(--muted); font-size: var(--text-13); }
  .benefits li { display: inline-flex; align-items: center; gap: 8px; }
  .machine { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .offline { color: var(--ochre); font: var(--text-12) var(--font-mono); }
  .synced { display: inline-flex; color: var(--muted); }
</style>

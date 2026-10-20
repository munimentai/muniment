<script>
  import { creationTitle } from './thread-title.js'
  import PanelPage from './ui/PanelPage.svelte'
  import PageHead from './ui/PageHead.svelte'
  import CardGrid from './ui/CardGrid.svelte'
  import CatalogCard from './ui/CatalogCard.svelte'
  import Button from './ui/Button.svelte'
  import {onMount, untrack} from 'svelte'
  import CatalogActions from './CatalogActions.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import StarterRow from './StarterRow.svelte'
  import { creationStarters } from './creation-prompts.js'
  import AgentCard from '../agents/AgentCard.svelte'
  import AgentImport from '../agents/AgentImport.svelte'
  import { agentMachine, agentRuntimes, emptyListing, listAgents } from '../agents/agent-runtime.js'
  import { machineName } from '../runtimes/runtimes.js'
  import { exportAgent } from '../agents/import.js'
  import { loadModes } from '../agents/permission-modes.js'
  // `onselect` opens the agent's chat, `onprofile` its profile alone and `onopenrun` a run's conversation.
  let {tauri, onselect, oncreate, onchange=()=>{}, onimported=()=>{}, pending=[], threadSummaries=[], onopen, agents=null, runtimeList=null, inventory=null, latestRuns=null, isArchived=()=>false, onaction=()=>{}, onprofile=()=>{}, onrun=()=>{}, onopenrun=null, onvisibility=null, onmanagemodels=()=>{}}=$props()
  let archived = $state(false)
  let importOpen = $state(false)
  // The machines an import can land on: this computer and each that answers.
  const importRuntimes = $derived(agentRuntimes(runtimeList).map(runtime => ({ id: runtime.id, name: machineName(runtime) })))
  const machineOf = (agent) => agent.primaryMachine ?? agentMachine(runtimeList, agent)
  let listing=$state(emptyListing()), error=$state(''), loading=$state(true)
  let alive = true
  onMount(()=>{listAgents(tauri, runtimeList).then(value=>{if(alive){listing=value;onchange(value)}}).catch(e=>error=String(e?.message ?? e)).finally(()=>loading=false);return()=>{alive=false}})
  // A card's change lists the agents again, so every card shows what core saved.
  async function refresh() {
    const value = await listAgents(tauri, runtimeList)
    if (!alive) return
    listing = value
    onchange(value)
  }
  // Each machine's permission modes, read once per machine the cards name.
  // A machine that lists none maps to null.
  let modes = $state({})
  $effect(() => {
    const runtimes = (agents ?? listing.agents).filter((agent) => !agent.replica && agent.online !== false).map((agent) => agent.runtimeId ?? 'local')
    untrack(() => {
      for (const runtime of new Set(runtimes)) {
        if (runtime in modes) continue
        modes[runtime] = undefined
        void loadModes(tauri, runtime).then((value) => { if (alive) modes[runtime] = value })
      }
    })
  })
  // The card menu opens the profile and exports here; the rest is the catalog's.
  async function cardAction(agent, action, name) {
    if (action === 'profile') return onprofile(agent)
    if (action === 'export') return exportAgent(tauri, agent, 'muniment')
    return onaction('agent', agent, action, name)
  }
</script>
<section data-panel="agents" class="panel" aria-label="Agents">
  <PanelPage>
    <PageHead title="Agents" level={2} intro="Each agent keeps its own instructions and memory, works in its own chat, and can run on a schedule, a webhook or a tool check.">
      {#snippet actions()}
        {#if archived || (agents ?? listing.agents).some(agent=>isArchived('agent',agent)) || pending.some(plan=>isArchived('creation',plan))}<Button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</Button>{/if}
        <Button icon="download" onclick={()=>importOpen=true}>Import agent</Button>
        <Button icon="plus" variant="primary" onclick={()=>oncreate?.()}>New agent</Button>
      {/snippet}
    </PageHead>
    {#if error}<p role="alert">{error}</p>{/if}
    {#if loading}<p role="status">Loading agents…</p>{/if}
    {#if !archived && !loading}
      {@const some = (agents ?? listing.agents).some(agent=>!isArchived('agent',agent)) || pending.some(plan=>!isArchived('creation',plan))}
      <StarterRow heading="Give an agent a job" kind="agent" items={creationStarters.agent} compact={some} oncreate={(item)=>oncreate?.({goal:item.goal,output:item.output,description:item.description})} />
    {/if}
    <CardGrid min={340} rows={300}>
      {#each (agents ?? listing.agents).filter(agent=>isArchived('agent',agent)===archived) as agent (`${agent.runtimeId ?? 'local'}:${agent.id}`)}
        <AgentCard {agent} {tauri} {inventory} {archived} modes={agent.replica || agent.online === false ? null : modes[agent.runtimeId ?? 'local']} machine={machineOf(agent)} latestRun={(latestRuns ?? listing.latestRuns)[agent.id] ?? null}
          onchat={onselect} {onprofile} {onrun} {onopenrun} {onvisibility} {onmanagemodels} onchange={refresh} onaction={(action,name)=>cardAction(agent,action,name)} />
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
</style>

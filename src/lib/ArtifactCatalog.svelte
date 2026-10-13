<script>
  import PanelPage from './ui/PanelPage.svelte'
  import PageHead from './ui/PageHead.svelte'
  import CardGrid from './ui/CardGrid.svelte'
  import CatalogCard from './ui/CatalogCard.svelte'
  import Button from './ui/Button.svelte'
  import CatalogActions from './CatalogActions.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import StarterRow from './StarterRow.svelte'
  import { creationStarters } from './creation-prompts.js'
  let {items=[],onopen,oncreate,isArchived=()=>false,onaction=()=>{}}=$props()
  let archived = $state(false)
  const shown = $derived(items.filter(item=>isArchived('artifact',item)===archived))
</script>
<section data-panel="artifacts" class="panel" aria-label="Artifacts">
  <h2 class="visually-hidden">Artifacts</h2>
  <PanelPage>
    <PageHead intro="Pages and apps your chats build, each kept with the chat that made it.">
      {#snippet actions()}
        {#if archived || items.some(item=>isArchived('artifact',item))}<Button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</Button>{/if}
        <Button icon="plus" onclick={()=>oncreate?.()}>New artifact</Button>
      {/snippet}
    </PageHead>
    {#if !archived}
      <StarterRow heading="Make something new" lede="Describe it in a chat, and your model builds it as one HTML file." kind="artifact" items={creationStarters.artifact} compact={shown.length > 0} oncreate={(item)=>oncreate?.({goal:item.goal,output:item.output})} />
      {#if !shown.length}
        <ul class="benefits">
          <li><LucideIcon name="cpu" />Built by the model you connect</li>
          <li><LucideIcon name="file-code" />One self-contained HTML file</li>
          <li><LucideIcon name="messages-square" />Revised in the chat that made it</li>
        </ul>
      {/if}
    {/if}
    <CardGrid>
      {#each shown as item (item.threadId || item.id)}
        <CatalogCard onclick={()=>onopen(item)}>
          {#snippet menu()}<CatalogActions name={item.name || item.goal} {archived} onaction={(action,name)=>onaction('artifact',item,action,name)}/>{/snippet}
          <LucideIcon name="file-code" size={32}/><strong>{item.name || item.goal}</strong><span>{item.output || 'Open the artifact chat'}</span>
        </CatalogCard>
      {/each}
    </CardGrid>
    {#if archived && !shown.length}<p>No archived artifacts.</p>{/if}
  </PanelPage>
</section>
<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .benefits { display: flex; flex-wrap: wrap; gap: 8px 24px; margin: 0; padding: 0; list-style: none; color: var(--muted); font-size: var(--text-13); }
  .benefits li { display: inline-flex; align-items: center; gap: 8px; }
</style>

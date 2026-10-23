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
  <PanelPage>
    <PageHead title="Artifacts" level={2} intro="Pages and apps your model builds as one self-contained HTML file. Each stays with the chat that made it, where you revise it.">
      {#snippet actions()}
        {#if archived || items.some(item=>isArchived('artifact',item))}<Button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</Button>{/if}
        <Button icon="plus" onclick={()=>oncreate?.()}>New artifact</Button>
      {/snippet}
    </PageHead>
    {#if !archived}
      <StarterRow heading="Make something new" kind="artifact" items={creationStarters.artifact} compact={shown.length > 0} oncreate={(item)=>oncreate?.({goal:item.goal,output:item.output})} />
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
</style>

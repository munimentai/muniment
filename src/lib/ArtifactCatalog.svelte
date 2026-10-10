<script>
  import PanelPage from './ui/PanelPage.svelte'
  import PageHead from './ui/PageHead.svelte'
  import CardGrid from './ui/CardGrid.svelte'
  import CatalogCard from './ui/CatalogCard.svelte'
  import Button from './ui/Button.svelte'
  import CatalogActions from './CatalogActions.svelte'
  import LucideIcon from './LucideIcon.svelte'
  let {items=[],onopen,oncreate,onclose,isArchived=()=>false,onaction=()=>{}}=$props()
  let archived = $state(false)
</script>
<section data-panel="artifacts" class="panel" aria-label="Artifacts">
  <h2 class="visually-hidden">Artifacts</h2>
  <PanelPage>
    <PageHead intro="Pages and apps your chats build, each kept with the chat that made it.">
      {#snippet actions()}
        <Button aria-pressed={archived} onclick={()=>archived=!archived}>{archived ? 'Show active' : 'Show archived'}</Button>
        <Button onclick={oncreate}>New artifact</Button>
        <button type="button" aria-label="Close artifacts" onclick={onclose}><LucideIcon name="x" /></button>
      {/snippet}
    </PageHead>
    <CardGrid>
      {#each items.filter(item=>isArchived('artifact',item)===archived) as item (item.threadId || item.id)}
        <CatalogCard onclick={()=>onopen(item)}>
          {#snippet menu()}<CatalogActions name={item.name || item.goal} {archived} onaction={(action,name)=>onaction('artifact',item,action,name)}/>{/snippet}
          <LucideIcon name="file-code" size={32}/><strong>{item.name || item.goal}</strong><span>{item.output || 'Open the artifact chat'}</span>
        </CatalogCard>
      {/each}
    </CardGrid>
    {#if !items.some(item=>isArchived('artifact',item)===archived)}<p>{archived ? 'No archived artifacts.' : 'No artifacts yet. New artifact starts one in a chat.'}</p>{/if}
  </PanelPage>
</section>
<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
</style>

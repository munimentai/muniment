<script>
  import { getContext } from 'svelte'
  import InfoTip from './InfoTip.svelte'
  // A page's first row: the page title with its explanation behind an info
  // control at the left, and the page's actions at the right. Settings names
  // the page through the `page-title` context, so its sections need not.
  // Below 560px an action keeps its icon and hides its .action-label. `lead`
  // puts a control, such as Back, before the title. A narrow page moves the
  // actions below the title.
  let { title = '', intro = '', level = 3, lead, actions } = $props()
  const contextTitle = getContext('page-title')
  const heading = $derived(title || contextTitle?.() || '')
  const id = $props.id()
</script>

<div class="page-head">
  {@render lead?.()}
  {#if heading}
    <div class="page-title-row">
      <svelte:element this={`h${level}`} class="page-title" aria-describedby={intro ? `${id}-info` : undefined}>{heading}</svelte:element>
      <InfoTip id={`${id}-info`} text={intro} label={`About ${heading}`} />
    </div>
  {:else if intro}
    <p class="settings-intro">{intro}</p>
  {/if}
  {#if actions}<div class="page-actions">{@render actions()}</div>{/if}
</div>

<style>
  .page-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 16px; min-height: 32px; container-type: inline-size; }
  .page-head > p { flex: 1 1 280px; min-width: 0; margin: 0; }
  .page-title-row { display: flex; flex: 1 1 200px; align-items: center; gap: 4px; min-width: 0; }
  .page-title { margin: 0; min-width: 0; overflow-wrap: anywhere; color: var(--ink); font: var(--weight-semibold) var(--text-22) / var(--leading-heading) var(--font-heading); letter-spacing: 0; }
  .page-actions { display: flex; flex: none; align-items: center; gap: 8px; }
  @container (max-width: 560px) { .page-actions :global(.action-label) { display: none; } }
</style>

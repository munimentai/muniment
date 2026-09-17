<script>
  // One record: the kind's prose first, then the core fields, the company's
  // own fields, the identities, the edges by relation with their validity
  // windows, and the history. Every value is a record and reads in mono;
  // the title and the prose read in the body face.
  import { cellText, groupEdges, tableColumns } from './record-table-state.js'

  let { detail, onopen } = $props()

  const entity = $derived(detail?.entity)
  const fields = $derived(tableColumns(detail?.kind).filter((column) => !column.base))
  // Only the fields that hold a value: an absent one is not a fact.
  const held = (column) => cellText(entity?.data?.[column.key], column) !== ''
  const coreFields = $derived(fields.filter((column) => !column.own && held(column)))
  const ownFields = $derived(fields.filter((column) => column.own && held(column)))
  const edgeGroups = $derived(groupEdges(detail?.edges, entity?.id))

  function when(text) {
    return cellText(text, { type: 'date-time' })
  }
</script>

{#if entity}
  <article class="record-view" aria-label={entity.title}>
    <h3 class="record-title">{entity.title}</h3>
    {#if entity.body_text}<p class="record-prose">{entity.body_text}</p>{/if}
    <p class="record-meta">{entity.kind} · {entity.id}{entity.state ? ` · ${entity.state}` : ''} · updated {when(entity.updated_at)}</p>

    <section class="record-section" aria-label="Fields">
      <h4>Fields</h4>
      <dl class="record-fields">
        {#each coreFields as column (column.key)}
          <dt>{column.label}</dt>
          <dd class:mono={column.mono}>{cellText(entity.data?.[column.key], column)}</dd>
        {/each}
      </dl>
      {#if ownFields.length}
        <h4>Own fields</h4>
        <dl class="record-fields">
          {#each ownFields as column (column.key)}
            <dt>{column.label}</dt>
            <dd class:mono={column.mono}>{cellText(entity.data?.[column.key], column)}</dd>
          {/each}
        </dl>
      {/if}
    </section>

    {#if detail.identities?.length}
      <section class="record-section" aria-label="Identities">
        <h4>Identities</h4>
        <ul class="record-list mono">
          {#each detail.identities as identity (identity.kind + identity.value)}
            <li>{identity.kind}: {identity.value}</li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if edgeGroups.length}
      <section class="record-section" aria-label="Relations">
        <h4>Relations</h4>
        {#each edgeGroups as group (group.relation)}
          <p class="record-relation">{group.relation}</p>
          <ul class="record-list">
            {#each group.items as item (item.id)}
              <li class:closed={!!item.validTo}>
                <button type="button" class="record-link" onclick={() => onopen?.(item.otherId)}>{item.otherTitle}</button>
                <span class="record-edge-meta">{item.otherKind} · from {when(item.validFrom)}{item.validTo ? ` to ${when(item.validTo)}` : ''}</span>
              </li>
            {/each}
          </ul>
        {/each}
      </section>
    {/if}

    <section class="record-section" aria-label="History">
      <h4>History</h4>
      <ol class="record-list mono">
        {#each detail.events ?? [] as event (event.id)}
          <li>{event.seq} · {when(event.at)} · {event.verb} · {event.actor_id}{event.on_behalf_of ? ` for ${event.on_behalf_of}` : ''}</li>
        {/each}
      </ol>
    </section>
  </article>
{/if}

<style>
  .record-view { min-height: 0; overflow-y: auto; padding-top: 12px; }
  .record-title { margin: 0; font: 600 var(--text-17)/1.3 var(--font-human); }
  .record-prose { margin: 8px 0 0; font: var(--text-15)/1.55 var(--font-human); }
  .record-meta { margin: 6px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .record-section { margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--border); }
  .record-section h4 { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); text-transform: none; }
  .record-fields { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 4px 16px; margin: 0; }
  .record-fields dt { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-fields dd { margin: 0; font: var(--text-13) var(--font-human); overflow-wrap: anywhere; }
  .record-fields dd.mono, .record-list.mono { font: var(--text-12) var(--font-mono); }
  .record-list { margin: 0; padding: 0; list-style: none; display: grid; gap: 4px; }
  .record-list li.closed { color: var(--muted); }
  .record-relation { margin: 8px 0 4px; font: var(--text-12) var(--font-mono); color: var(--ink); }
  .record-link { padding: 0; border: 0; background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; text-align: left; }
  .record-link:hover { text-decoration: underline; }
  .record-edge-meta { margin-left: 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
</style>

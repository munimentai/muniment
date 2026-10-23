<script>
  // Settings → Machines: what this computer still takes from your other
  // machines. While a bootstrap fills it, each class of records shows a bar
  // from core's fleet sync status, and each model sign-in it cannot use yet
  // says where it arrives from or why it stays on its own machine. The group
  // shows only while one of them is listed.
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import { bootstrapRows, credentialRows, listsArrived } from './fleet.js'

  let { status = null, credentials = null, swarm = null } = $props()
  const bootstrapping = $derived(status?.state === 'bootstrapping')
  const rows = $derived(bootstrapping ? bootstrapRows(status) : [])
  const waiting = $derived(credentialRows(credentials, swarm))
</script>

{#if rows.length || waiting.length}
  <Group heading="Sync" description={bootstrapping ? (listsArrived(status) ? 'Lists arrived. History keeps filling.' : 'Getting this computer up to date.') : ''}>
    {#if rows.length}
      <ul class="classes" aria-label="Sync progress">
        {#each rows as row (row.id)}
          <li class:done={row.done}>
            <span class="class-name">{row.name}</span>
            <span class="class-count">{row.total ? `${row.held}/${row.total}` : row.done ? 'done' : '…'}</span>
            <span class="bar" role="progressbar" aria-label={row.name} aria-valuemin="0" aria-valuemax="100" aria-valuenow={row.percent}><span style:width="{row.percent}%"></span></span>
          </li>
        {/each}
      </ul>
    {/if}
    {#each waiting as account (account.id)}
      <Row title={account.name} description={account.line} />
    {/each}
  </Group>
{/if}

<style>
  .classes { display: grid; gap: 6px; margin: 0; padding: 10px 0; list-style: none; }
  .classes li { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 2px 8px; color: var(--ink); font: var(--text-12) var(--font-mono); line-height: 16px; }
  .class-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .class-count { color: var(--muted); white-space: nowrap; }
  .bar { grid-column: 1 / -1; display: block; height: 3px; border-radius: var(--radius-chip); background: var(--faint); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--muted); }
  .done .bar span { background: var(--success); }
</style>

<script>
  // The sidebar footer's sync line: Synced, Syncing N, or Offline with the
  // time of the last sync, from core's fleet sync status and the actions it
  // forwarded. It opens the bootstrap panel, which a machine that joins a
  // fleet shows on its own until the lists arrive.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { bootstrapRows, credentialRows, listsArrived, syncLine } from './fleet.js'

  let { status = null, ops = [], credentials = null, swarm = null } = $props()
  const line = $derived(syncLine(status, ops))
  const bootstrapping = $derived(status?.state === 'bootstrapping')
  const rows = $derived(bootstrapRows(status))
  const waiting = $derived(credentialRows(credentials, swarm))
  const usable = $derived(listsArrived(status))
  let shown = $state(null)
  // A bootstrap opens the panel until the user closes it.
  const open = $derived(shown ?? bootstrapping)
</script>

{#if line}
  <div class="sync">
    {#if open && (rows.length || waiting.length)}
      <section class="bootstrap" aria-label="Sync progress">
        <p class="bootstrap-head">{bootstrapping ? (usable ? 'Lists arrived. History keeps filling.' : 'Getting this computer up to date.') : 'Everything this computer holds.'}</p>
        <ul class="classes">
          {#each rows as row (row.id)}
            <li class:done={row.done}>
              <span class="class-name">{row.name}</span>
              <span class="class-count">{row.total ? `${row.held}/${row.total}` : row.done ? 'done' : '…'}</span>
              <span class="bar" role="progressbar" aria-label={row.name} aria-valuemin="0" aria-valuemax="100" aria-valuenow={row.percent}><span style:width="{row.percent}%"></span></span>
            </li>
          {/each}
        </ul>
        {#if waiting.length}
          <p class="bootstrap-sub">Sign-ins</p>
          <ul class="classes">
            {#each waiting as account (account.id)}
              <li class="account"><span class="class-name">{account.name}</span><span class="class-count" class:waiting={!account.online}>{account.line}</span></li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
    <button type="button" class="sync-line" data-state={line.state} aria-expanded={open} aria-label="{line.text}. {open ? 'Hide' : 'Show'} sync progress" onclick={() => { shown = !open }}>
      <LucideIcon name={line.icon} size={14} variant="action" /><span>{line.text}</span>
    </button>
  </div>
{/if}

<style>
  .sync { display: grid; gap: 4px; }
  .sync-line { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 24px; padding: 2px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-12) var(--font-human); text-align: left; cursor: pointer; }
  .sync-line:hover, .sync-line:focus-visible { background: var(--faint); color: var(--ink); }
  .sync-line span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sync-line[data-state="offline"] :global(.lucide) { color: var(--ochre); }
  .bootstrap { display: grid; gap: 6px; margin: 4px 2px 0; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  .bootstrap-head { margin: 0; color: var(--ink); font: var(--text-12) var(--font-human); line-height: 1.4; }
  .bootstrap-sub { margin: 4px 0 0; color: var(--muted); font: var(--text-12) var(--font-human); }
  .classes { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .classes li { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 2px 8px; color: var(--ink); font: var(--text-12) var(--font-mono); line-height: 16px; }
  .class-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .class-count { color: var(--muted); white-space: nowrap; }
  .class-count.waiting { color: var(--ochre); }
  .classes li.account { grid-template-columns: minmax(0, 1fr); }
  .bar { grid-column: 1 / -1; display: block; height: 3px; border-radius: var(--radius-chip); background: var(--faint); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--muted); }
  .done .bar span { background: var(--ink); }
</style>

<script>
  // Machines → This computer → Log: the fleet audit log from core's `fleet.audit`, newest
  // first. Each entry is a record row in mono: when, what happened, the
  // machines it moved between and its detail, with machine names from
  // `swarm.members` and connection names from `fleet.credentials` and the
  // router settings' decision models. The log shows in a fleet, or while this computer holds
  // entries from one. A runtime that does not answer `fleet.audit` keeps the
  // log hidden.
  import { onMount, tick } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { LOCAL_RUNTIME_ID } from './runtimes.js'
  import { AUDIT_LIMIT, auditRow } from './fleet.js'
  import { readFleetAccounts } from '../lib/fleet-accounts.js'

  let { tauri } = $props()
  const PAGE = 50
  let entries = $state(null)
  let more = $state(false)
  let limit = $state(PAGE)
  let busy = $state(false)
  let error = $state('')
  let alone = $state(true)
  // The members name the machines behind each entry's runtime ids.
  let swarm = $state(null)
  let accounts = $state(null)
  let decisionModels = $state(null)
  const rows = $derived((entries ?? []).map((entry) => auditRow(entry, swarm, accounts, decisionModels)))
  const shown = $derived(!!entries && (!alone || entries.length > 0))
  const ask = (operation, body = {}) => agentRequest(tauri, LOCAL_RUNTIME_ID, operation, body)

  async function load() {
    busy = true
    try {
      const [answer, members, status, fleet, router] = await Promise.all([
        ask('fleet.audit', { limit }),
        ask('swarm.members').catch(() => null),
        ask('fleet.sync_status').catch(() => null),
        readFleetAccounts(tauri),
        Promise.resolve().then(() => tauri?.invoke('model_router_settings')).catch(() => null),
      ])
      if (members?.members) swarm = members
      if (fleet) accounts = fleet
      if (Array.isArray(router?.classifier_connections)) decisionModels = router.classifier_connections
      alone = (status?.state ?? 'alone') === 'alone'
      entries = answer?.entries ?? []
      more = !!answer?.more && limit < AUDIT_LIMIT
      error = ''
    } catch (failure) {
      if (entries) error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }

  function showMore() {
    limit = Math.min(limit + PAGE, AUDIT_LIMIT)
    void load()
  }

  // The page reads its own lists first, then the log.
  onMount(() => { void tick().then(load) })
</script>

{#if shown}
  <Group heading="Log" count={rows.length} info="Seals, forwarded actions, role changes and removals across your machines.">
    {#snippet actions()}<Button icon="refresh-cw" disabled={busy} onclick={load}>Refresh</Button>{/snippet}
    {#if rows.length}
      <ol class="audit" aria-label="Machine log">
        {#each rows as row (row.id)}
          <li>
            <time datetime={row.at}>{row.time}</time>
            <span class="what">{row.what}</span>
            <span class="who">{row.machines}</span>
            {#if row.detail}<span class="detail">{row.detail}</span>{/if}
          </li>
        {/each}
      </ol>
    {:else}
      <p>No entries yet.</p>
    {/if}
    {#if more}<div class="more"><Button disabled={busy} onclick={showMore}>Show more</Button></div>{/if}
  </Group>
  {#if error}<p class="audit-error" role="alert">{error}</p>{/if}
{/if}

<style>
  /* The rows share the list's columns, so the time column takes the widest time and never runs into the event. */
  .audit { display: grid; grid-template-columns: max-content minmax(0, 1fr) auto; margin: 0; padding: 0; list-style: none; }
  .audit li { display: grid; grid-column: 1 / -1; grid-template-columns: subgrid; gap: 2px 12px; padding: 8px 0; color: var(--ink); font: var(--text-12) var(--font-mono); line-height: 16px; }
  .audit li + li { border-top: 1px solid var(--border); }
  .audit time, .who { color: var(--muted); white-space: nowrap; }
  .what { min-width: 0; overflow-wrap: anywhere; }
  .detail { grid-column: 2 / -1; color: var(--muted); overflow-wrap: anywhere; }
  .more { padding: 8px 0; }
  .audit-error { margin: 0; color: var(--ink); font-size: var(--text-13); }
</style>

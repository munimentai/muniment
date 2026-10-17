<script>
  // Machines → Log: the fleet's audit log from core's `fleet.audit`, newest
  // first. Each entry is a record row in mono: when, what happened, the
  // machine that acted and the machine it concerned. A runtime outside a
  // fleet refuses, and the log stays hidden.
  import { onMount, tick } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { LOCAL_RUNTIME_ID } from './runtimes.js'
  import { auditRow } from './fleet.js'

  let { tauri } = $props()
  const PAGE = 50
  let entries = $state(null)
  let more = $state(false)
  let limit = $state(PAGE)
  let busy = $state(false)
  let error = $state('')
  // The members name the machines behind each entry's runtime ids.
  let swarm = $state(null)
  const rows = $derived((entries ?? []).map((entry) => auditRow(entry, swarm)))

  async function load() {
    busy = true
    try {
      const [answer, members] = await Promise.all([
        agentRequest(tauri, LOCAL_RUNTIME_ID, 'fleet.audit', { limit }),
        agentRequest(tauri, LOCAL_RUNTIME_ID, 'swarm.members').catch(() => null),
      ])
      if (members?.members) swarm = members
      entries = answer?.entries ?? []
      more = !!answer?.more
      error = ''
    } catch (failure) {
      if (entries) error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }

  // The page reads its own lists first, then the log.
  onMount(() => { void tick().then(load) })
</script>

{#if entries}
  <Group heading="Log" count={rows.length} description="Seals, forwarded actions, role changes and removals across the fleet.">
    {#snippet actions()}<Button icon="refresh-cw" disabled={busy} onclick={load}>Refresh</Button>{/snippet}
    {#if rows.length}
      <ol class="audit" aria-label="Fleet log">
        {#each rows as row (row.id)}
          <li>
            <time datetime={row.at}>{row.time}</time>
            <span class="what">{row.what}</span>
            <span class="who">{row.machine && row.machine !== row.actor ? `${row.actor} → ${row.machine}` : row.actor}</span>
            {#if row.detail}<span class="detail">{row.detail}</span>{/if}
          </li>
        {/each}
      </ol>
    {:else}
      <p>No entries yet.</p>
    {/if}
    {#if more}<div class="more"><Button disabled={busy} onclick={() => { limit += PAGE; void load() }}>Show more</Button></div>{/if}
  </Group>
  {#if error}<p class="audit-error" role="alert">{error}</p>{/if}
{/if}

<style>
  .audit { display: grid; margin: 0; padding: 0; list-style: none; }
  .audit li { display: grid; grid-template-columns: 128px minmax(0, 1fr) auto; gap: 2px 12px; padding: 8px 0; color: var(--ink); font: var(--text-12) var(--font-mono); line-height: 16px; }
  .audit li + li { border-top: 1px solid var(--border); }
  .audit time, .who { color: var(--muted); white-space: nowrap; }
  .what { min-width: 0; overflow-wrap: anywhere; }
  .detail { grid-column: 2 / -1; color: var(--muted); overflow-wrap: anywhere; }
  .more { padding: 8px 0; }
  .audit-error { margin: 0; color: var(--ink); font-size: var(--text-13); }
</style>

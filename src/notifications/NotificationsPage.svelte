<script>
  // Notifications takes the main panel: what needs the user, pinned first,
  // then the runs in flight, then every other run by day. Core builds the
  // page from every machine that answers. The page asks again when a filter
  // changes, when `refreshKey` moves on a run or approval event, and every
  // half minute while it shows.
  import { onMount } from 'svelte'
  import PanelPage from '../lib/ui/PanelPage.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import ChoiceField from '../lib/ui/ChoiceField.svelte'
  import SegmentedField from '../lib/ui/SegmentedField.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import NeedsYouItem from './NeedsYouItem.svelte'
  import NotificationRow from './NotificationRow.svelte'
  import MachineChip from '../runtimes/MachineChip.svelte'
  import { machineRefs } from '../usage/usage.js'
  import { NOTIFICATION_VIEWS, dayLabel, markAllRead, readNotifications, runWithMachine, scheduleTime } from './notifications.js'

  let { tauri, runtimeList = null, agents = [], swarm = null, refreshKey = 0, onopen = () => {}, onapprovalopen = null, onread = () => {} } = $props()
  let view = $state('all')
  let machine = $state('')
  let agent = $state('')
  let page = $state(null)
  let loading = $state(false)
  let error = $state('')
  let now = $state(Date.now())
  const machines = $derived(machineRefs(runtimeList))
  const showMachine = $derived(machines.length > 1)
  const machineOptions = $derived([{ value: '', label: 'All machines' }, ...machines.map((entry) => ({ value: entry.id, label: entry.name }))])
  const agentOptions = $derived([{ value: '', label: 'All agents' }, ...(page?.agents ?? []).map((entry) => ({ value: entry.id, label: entry.name || 'Agent' }))])
  const quiet = $derived(page && !page.needs_you.length)
  let request = 0

  async function refresh() {
    const ticket = ++request
    loading = true
    try {
      const next = await readNotifications(tauri, machines, { view, machine: machine || null, agent: agent || null })
      if (ticket !== request) return
      page = next
      error = ''
      now = Date.now()
    } catch (reason) {
      if (ticket === request) error = String(reason?.message ?? reason)
    } finally {
      if (ticket === request) loading = false
    }
  }

  async function readAll() {
    try {
      await markAllRead(tauri)
      onread()
      await refresh()
    } catch (reason) {
      error = String(reason?.message ?? reason)
    }
  }

  $effect(() => { void view; void machine; void agent; void refreshKey; void machines.map((entry) => entry.id).join(); void refresh() })
  onMount(() => {
    const poll = () => { if (!document.hidden) void refresh() }
    const timer = setInterval(poll, 30_000)
    return () => clearInterval(timer)
  })
  const agentFor = (id) => agents.find((entry) => entry.id === id) ?? { id, name: '' }
</script>

<section data-panel="notifications" class="notifications-page" aria-label="Notifications">
  <PanelPage>
    <div class="head">
      <div class="crumbs">
        <h2>Notifications</h2>
        {#if showMachine}<span class="sep" aria-hidden="true">›</span><span class="filter"><ChoiceField label="Machine" hideLabel inline={false} value={machine} options={machineOptions} onchange={(value) => { machine = value }} /></span>{/if}
        <span class="sep" aria-hidden="true">›</span>
        <span class="filter"><ChoiceField label="Agent" hideLabel inline={false} value={agent} options={agentOptions} onchange={(value) => { agent = value }} /></span>
      </div>
      <div class="controls">
        <SegmentedField label="Show" hideLabel value={view} options={NOTIFICATION_VIEWS} onchange={(value) => { view = value }} />
        <Button icon="check-check" disabled={!page?.counts?.unread} onclick={readAll}>Mark all read</Button>
      </div>
    </div>

    {#if error}<p class="note error" role="alert">{error}</p>{/if}
    {#if !page && !error}<p class="note" role="status">{loading ? 'Reading notifications…' : ''}</p>{/if}

    {#if page}
      {#if quiet}
        <div class="quiet">
          <p class="quiet-line"><LucideIcon name="circle-check" size={16} variant="action" />Nothing needs you</p>
          {#if page.upcoming.length}
            <ul class="upcoming" aria-label="Next scheduled runs">
              {#each page.upcoming as next (`${next.agent_id}:${next.routine_id}`)}
                <li><LucideIcon name="clock" size={14} /><span>{next.agent_name}</span><span class="muted">{next.routine_name}</span>{#if showMachine && next.machine}<MachineChip machine={next.machine} {runtimeList} />{/if}<time datetime={new Date(next.at_ms).toISOString()}>{scheduleTime(next.at_ms)}</time></li>
              {/each}
            </ul>
          {/if}
        </div>
      {:else}
        <Group heading="Needs you" count={page.counts.needs_you || null}>
          {#each page.needs_you as item (`${item.kind}:${item.id}`)}
            <NeedsYouItem {tauri} {item} {agents} {swarm} {runtimeList} {showMachine} {now} {onopen} {onapprovalopen} onanswered={() => void refresh()} />
          {/each}
        </Group>
      {/if}

      {#if page.running.length}
        <Group heading="Running" count={page.running.length} plain>
          <ul class="running" aria-label="Running now">
            {#each page.running as entry (`${entry.machine.id}:${entry.run.id}`)}
              <li><button type="button" disabled={!entry.run.thread_id} onclick={() => onopen(runWithMachine(entry.run, entry.machine))}>
                <span class="live"><LucideIcon name="loader-circle" size={14} variant="action" /></span>
                <AgentAvatar agent={agentFor(entry.run.agent_id)} size={18} tooltip={false} />
                <span>{entry.agent_name || 'Agent'}</span>
                {#if showMachine}<MachineChip machine={entry.machine} {runtimeList} />{/if}
              </button></li>
            {/each}
          </ul>
        </Group>
      {/if}

      {#each page.days as day (day.day)}
        <Group heading={dayLabel(day.day)} plain>
          <ul class="rows">{#each day.entries as entry (`${entry.machine.id}:${entry.run.id}`)}<NotificationRow {entry} {showMachine} {runtimeList} {onopen} />{/each}</ul>
        </Group>
      {:else}
        {#if view !== 'needs_you'}<p class="note">{view === 'failed' ? 'No failed runs.' : 'No runs yet.'}</p>{/if}
      {/each}
    {/if}
  </PanelPage>
</section>

<style>
  .notifications-page { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  .head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 10px 16px; }
  .crumbs, .controls { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; }
  h2 { margin: 0; color: var(--ink); font: 600 var(--text-17) var(--font-heading, var(--font-human)); }
  .sep { color: var(--muted); }
  .filter { width: 170px; }
  .note { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .note.error { color: var(--danger); }
  .quiet { display: grid; gap: 10px; }
  .quiet-line { display: flex; align-items: center; gap: 8px; margin: 0; color: var(--ink); font-size: var(--text-15); }
  .quiet-line :global(.lucide) { color: var(--success); }
  .upcoming, .running, .rows { display: grid; margin: 0; padding: 0; list-style: none; }
  .upcoming li { display: flex; align-items: center; gap: 8px; min-height: 28px; color: var(--ink); font-size: var(--text-13); }
  .upcoming time { margin-left: auto; color: var(--muted); font-size: var(--text-12); }
  .muted { color: var(--muted); }
  .running { display: flex; flex-wrap: wrap; gap: 8px; }
  .running button { display: inline-flex; align-items: center; gap: 6px; min-height: 30px; padding: 4px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; }
  .running button:hover:not(:disabled) { background: var(--faint); }
  .live { display: inline-flex; color: var(--active); }
  :global(.notifications-page .group-card) { padding: 0 12px; }
</style>

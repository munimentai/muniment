<script>
  // Activity: approvals waiting on any machine of the swarm, then every
  // agent's runs on every machine, newest first. The window keeps both
  // current from each machine's `agent.run_updated` and approval events.
  import PanelPage from '../lib/ui/PanelPage.svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import RunList from './RunList.svelte'
  import ApprovalList from './ApprovalList.svelte'
  import { pairedRuntimes } from '../runtimes/runtimes.js'

  let { tauri = null, runs = [], agents = [], approvals = [], swarm = null, runtimeList = null, live = {}, loading = false, error = '', unreachable = [], onrefresh, onopen, onagents = null, onanswered = () => {}, onopenapproval = null } = $props()
  const machines = $derived(pairedRuntimes(runtimeList).length > 0 || (swarm?.members?.length ?? 0) > 1)
  const offline = $derived(unreachable.map((id) => runtimeList?.runtimes?.find((runtime) => runtime.id === id)?.name).filter(Boolean))
  const quiet = $derived(!loading && !error && runs.length === 0 && approvals.length === 0)
</script>

<section data-panel="activity" class="panel" aria-label="Activity">
  <h2 class="visually-hidden">Activity</h2>
  <PanelPage>
    <PageHead intro="Runs from every agent on every machine, newest first.">
      {#snippet actions()}<Button icon="refresh-cw" disabled={loading} onclick={onrefresh}>Refresh</Button>{/snippet}
    </PageHead>
    {#if error}<p role="alert">{error}</p>{/if}
    {#if offline.length}<p class="note">{offline.join(', ')} did not answer. Its runs show when it is back.</p>{/if}
    {#if quiet}
      <div class="empty-state">
        <p>No agent runs yet.</p>
        {#if onagents}<Button icon="bot" onclick={onagents}>{agents.length ? 'Open agents' : 'Create an agent'}</Button>{/if}
      </div>
    {:else}
      {#if approvals.length}
        <Group heading="Waiting for approval" count={approvals.length}>
          <ApprovalList {tauri} {approvals} {agents} {swarm} {runtimeList} {live} onopen={onopenapproval} {onanswered} />
        </Group>
      {/if}
      <Group heading="Runs" count={runs.length}>
        <RunList {runs} {agents} {runtimeList} showAgent showMachine={machines} empty={loading ? 'Loading runs…' : 'No runs yet.'} {onopen} />
      </Group>
    {/if}
  </PanelPage>
</section>

<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  .note, [role="alert"] { margin: 0; color: var(--muted); font-size: var(--text-13); }
  [role="alert"] { color: var(--oxide); }
  .empty-state { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; }
  .empty-state p { margin: 0; color: var(--muted); font-size: var(--text-13); }
</style>

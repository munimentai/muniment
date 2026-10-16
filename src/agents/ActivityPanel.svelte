<script>
  // Activity: every agent's runs on every machine, newest first. The window
  // keeps the list current from each machine's `agent.run_updated` events.
  import PanelPage from '../lib/ui/PanelPage.svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import RunList from './RunList.svelte'
  import { pairedRuntimes } from '../runtimes/runtimes.js'

  let { runs = [], agents = [], runtimeList = null, loading = false, error = '', unreachable = [], onrefresh, onopen } = $props()
  const machines = $derived(pairedRuntimes(runtimeList).length > 0)
  const offline = $derived(unreachable.map((id) => runtimeList?.runtimes?.find((runtime) => runtime.id === id)?.name).filter(Boolean))
</script>

<section data-panel="activity" class="panel" aria-label="Activity">
  <h2 class="visually-hidden">Activity</h2>
  <PanelPage>
    <PageHead intro="Runs from every agent on every machine, newest first.">
      {#snippet actions()}<Button icon="refresh-cw" disabled={loading} onclick={onrefresh}>Refresh</Button>{/snippet}
    </PageHead>
    {#if error}<p role="alert">{error}</p>{/if}
    {#if offline.length}<p class="note">{offline.join(', ')} did not answer. Its runs show when it is back.</p>{/if}
    <Group heading="Runs" count={runs.length}>
      <RunList {runs} {agents} {runtimeList} showAgent showMachine={machines} empty={loading ? 'Loading runs…' : 'No agent runs yet.'} {onopen} />
    </Group>
  </PanelPage>
</section>

<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  .note, [role="alert"] { margin: 0; color: var(--muted); font-size: var(--text-13); }
  [role="alert"] { color: var(--oxide); }
</style>

<script>
  // Usage takes the main panel: what each account's subscription windows
  // have left, and what the runs on every machine used, by provider, model,
  // agent, project and machine. Core sums and reads; the page asks again when
  // a filter changes and every minute while it shows. Refresh asks each
  // account's upstream first.
  import { onMount } from 'svelte'
  import PanelPage from '../lib/ui/PanelPage.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import ChoiceField from '../lib/ui/ChoiceField.svelte'
  import SegmentedField from '../lib/ui/SegmentedField.svelte'
  import InfoTip from '../lib/ui/InfoTip.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import LimitCard from './LimitCard.svelte'
  import UsageChart from './UsageChart.svelte'
  import UsageBreakdown from './UsageBreakdown.svelte'
  import { accountCache } from '../lib/account-cache.js'
  import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'
  import { BREAKDOWNS, USAGE_RANGES, USAGE_VIEWS, machineRefs, providerLabel, providerOptions, readUsage, totalTiles, unmeteredLabel } from './usage.js'

  let { tauri, runtimeList = null, folderAccess = null, onconnect = null } = $props()
  let view = $state('limits')
  let range = $state('7d')
  let machine = $state('')
  let provider = $state('')
  let breakdown = $state('model')
  let summary = $state(null)
  let loading = $state(false)
  let error = $state('')
  let now = $state(Date.now())
  const machines = $derived(machineRefs(runtimeList))
  const asked = $derived(machine ? machines.filter((entry) => entry.id === machine) : machines)
  const machineOptions = $derived([{ value: '', label: 'All machines' }, ...machines.map((entry) => ({ value: entry.id, label: entry.name }))])
  const failed = $derived((summary?.machines ?? []).filter((entry) => entry.state !== 'ok'))
  const rows = $derived(breakdown === 'machine' ? summary?.breakdown?.machine ?? [] : summary?.breakdown?.[breakdown] ?? [])
  const breakdowns = $derived(machines.length > 1 ? BREAKDOWNS : BREAKDOWNS.filter((entry) => entry.value !== 'machine'))
  let request = 0

  async function refresh() {
    const ticket = ++request
    loading = true
    try {
      const next = await readUsage(tauri, asked, { range, provider: provider || null })
      if (ticket !== request) return
      summary = next
      error = ''
      now = Date.now()
    } catch (reason) {
      if (ticket === request) error = String(reason?.message ?? reason)
    } finally {
      if (ticket === request) loading = false
    }
  }

  async function refreshNow() {
    loading = true
    try { await accountCache(tauri).refresh() } catch (_) {}
    await refresh()
  }

  $effect(() => { void range; void provider; void asked.map((entry) => entry.id).join(); void refresh() })
  onMount(() => {
    const poll = () => { if (!document.hidden) void refresh() }
    const timer = setInterval(poll, 60_000)
    document.addEventListener('visibilitychange', poll)
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', poll) }
  })
</script>

<section data-panel="usage" class="usage-page" aria-label="Usage">
  <PanelPage>
    <div class="usage-head">
      <div class="crumbs">
        <h2>Usage</h2>
        <span class="sep" aria-hidden="true">›</span>
        <span class="filter"><ChoiceField label="Machine" hideLabel inline={false} value={machine} options={machineOptions} onchange={(value) => { machine = value; if (breakdown === 'machine' && value) breakdown = 'model' }} /></span>
        <span class="sep" aria-hidden="true">›</span>
        <span class="filter"><ChoiceField label="Provider" hideLabel inline={false} value={provider} options={providerOptions(summary)} onchange={(value) => { provider = value }} /></span>
      </div>
      <div class="controls">
        <SegmentedField label="Usage view" hideLabel value={view} options={USAGE_VIEWS} onchange={(value) => { view = value }} />
        {#if view !== 'limits'}<SegmentedField label="Range" hideLabel value={range} options={USAGE_RANGES} onchange={(value) => { range = value }} />{/if}
        <Button icon="refresh-cw" disabled={loading} onclick={refreshNow} aria-label="Refresh usage">Refresh</Button>
      </div>
    </div>

    {#if error}<p class="note error" role="alert">{error}</p>{/if}
    {#each failed as entry (entry.id)}<p class="note" role="status"><LucideIcon name="globe-off" size={14} />{entry.id === LOCAL_RUNTIME_ID && folderAccess ? folderAccess.message : `${entry.name} did not answer. The totals leave it out.`}</p>{/each}

    {#if !summary && !error}
      <p class="note" role="status">Reading usage…</p>
    {:else if summary && view === 'limits'}
      {#if summary.limits.length}
        <div class="limits">{#each summary.limits as limit (`${limit.provider}:${limit.account_id ?? ''}:${limit.kind}:${limit.scope}`)}<LimitCard {limit} {now} />{/each}</div>
      {:else}
        <div class="empty"><p>No subscription reports a limit yet.</p>{#if onconnect}<Button icon="plus" variant="primary" onclick={onconnect}>Connect account</Button>{/if}</div>
      {/if}
      {#if summary.unmetered.length}
        <Group heading="Without a window">
          {#each summary.unmetered as entry (`${entry.provider}:${entry.reason}`)}
            <div class="unmetered"><ProviderLogo provider={entry.provider} size={16} color /><span>{entry.label || providerLabel(entry.provider)}</span><em>{unmeteredLabel(entry)}</em></div>
          {/each}
        </Group>
      {/if}
    {:else if summary}
      <dl class="tiles">
        {#each totalTiles(summary.totals, view) as tile (tile.label)}<div><dt>{tile.label}</dt><dd>{tile.value}</dd></div>{/each}
      </dl>
      {#if view === 'cost'}<p class="estimate">API estimate from each model's list price. Subscriptions bill differently.<InfoTip text="Each finished run's receipt carries the catalog price of its tokens. A run that failed or stopped keeps no receipt." label="About the estimate" /></p>{/if}
      <Group heading={view === 'cost' ? 'Cost by provider' : 'Tokens by provider'} plain>
        {#if summary.totals.runs}<UsageChart series={summary.series} bucket={summary.bucket} {view} />{:else}<p class="note">No finished runs in this range.</p>{/if}
      </Group>
      <Group heading="Breakdown" plain>
        {#snippet actions()}<SegmentedField label="Breakdown" hideLabel value={breakdown} options={breakdowns} onchange={(value) => { breakdown = value }} />{/snippet}
        <UsageBreakdown {rows} {breakdown} {view} />
      </Group>
    {/if}
  </PanelPage>
</section>

<style>
  .usage-page { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; container: usage / inline-size; }
  .usage-head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 10px 16px; }
  .crumbs, .controls { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; }
  h2 { margin: 0; color: var(--ink); font: 600 var(--text-17) var(--font-heading, var(--font-human)); }
  .sep { color: var(--muted); }
  .filter { width: 170px; }
  .note { display: flex; align-items: center; gap: 6px; margin: 0; color: var(--muted); font-size: var(--text-13); }
  .note.error { color: var(--danger); }
  .limits { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(240px, 100%), 1fr)); gap: 12px; }
  .empty { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; }
  .empty p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .unmetered { display: flex; align-items: center; gap: 8px; min-height: 36px; color: var(--ink); font-size: var(--text-13); }
  .unmetered span { flex: 1; }
  .unmetered em { color: var(--muted); font-style: normal; font-size: var(--text-12); }
  .tiles { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 12px; margin: 0; }
  .tiles div { min-width: 0; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  dt { color: var(--muted); font-size: var(--text-12); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  dd { margin: 4px 0 0; color: var(--ink); font: 600 var(--text-17) var(--font-mono); font-variant-numeric: tabular-nums; }
  .estimate { display: flex; align-items: center; gap: 6px; margin: -6px 0 0; color: var(--muted); font-size: var(--text-12); }
  @container usage (max-width: 720px) { .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>

<script>
  // Agent runs, newest first: state, trigger, duration and a silent mark,
  // with the agent and its machine when the list spans agents.
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import MachineMark from './MachineMark.svelte'
  import StatusChip from '../lib/ui/StatusChip.svelte'
  import { runStatusTone } from '../lib/status-tone.js'
  import { runActive, runDuration, runNote, runStatusLabel, triggerLabel, whenLabel } from '../lib/agent-labels.js'
  import { agentMachine } from './agent-runtime.js'

  let { runs = [], agents = [], runtimeList = null, showAgent = false, showMachine = false, empty = 'No runs yet.', onopen = null } = $props()
  let now = $state(Date.now())
  $effect(() => {
    if (!runs.some((run) => runActive(run.status))) return
    const timer = setInterval(() => { now = Date.now() }, 1000)
    return () => clearInterval(timer)
  })
  const agentFor = (run) => agents.find((agent) => agent.id === run.agent_id && (agent.runtimeId ?? 'local') === (run.runtimeId ?? 'local'))
</script>

<ul class="runs" aria-label="Runs">
  {#each runs as run (`${run.runtimeId ?? 'local'}:${run.id}`)}
    {@const agent = agentFor(run)}
    <li class="run">
      {#if showAgent}<span class="who">{#if agent}<AgentAvatar {agent} size={20} />{/if}<strong>{agent?.name ?? 'Removed agent'}</strong></span>{/if}
      <StatusChip tone={runStatusTone(run.status)}>{runStatusLabel(run.status)}</StatusChip>
      <span class="meta">
        <span>{triggerLabel(run.trigger?.kind)}</span>
        {#if runDuration(run, now)}<span>{runDuration(run, now)}</span>{/if}
        {#if run.silent}<span class="silent">silent</span>{/if}
        <time datetime={run.updated_at}>{whenLabel(run.updated_at, now)}</time>
        {#if showMachine}<MachineMark machine={agentMachine(runtimeList, { runtimeId: run.runtimeId })} />{/if}
      </span>
      {#if runNote(run)}<p class="note">{runNote(run)}</p>{/if}
      {#if onopen && run.thread_id}<button type="button" class="open" onclick={() => onopen(run)}>Open conversation</button>{/if}
    </li>
  {:else}
    <li class="empty">{empty}</li>
  {/each}
</ul>

<style>
  .runs { display: grid; margin: 0; padding: 0; list-style: none; }
  .run { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 10px; padding: 8px 0; border-bottom: 1px solid var(--border); }
  .run:last-child { border-bottom: 0; }
  .who { display: inline-flex; align-items: center; gap: 6px; min-width: 0; font-size: var(--text-13); }
  .meta { display: inline-flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; min-width: 0; color: var(--muted); font: var(--text-12) var(--font-human); }
  .silent { padding: 0 4px; border: 1px solid var(--border); border-radius: var(--radius-chip); }
  .note { flex-basis: 100%; margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.5; overflow-wrap: anywhere; }
  .open { min-height: 24px; padding: 2px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; font-size: var(--text-12); cursor: pointer; }
  .open:hover { background: var(--faint); }
  .empty { padding: 8px 0; color: var(--muted); font-size: var(--text-12); }
</style>

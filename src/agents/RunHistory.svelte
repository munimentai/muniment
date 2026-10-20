<script module>
  import { runActive, runLight } from '../lib/agent-labels.js'

  // A run's status icon, drawn in its stoplight color.
  const ICONS = { green: 'circle-check', red: 'circle-x' }
  export const runIcon = (run) => run.status === 'waiting_input' ? 'circle-alert' : runActive(run.status) ? 'loader-circle' : ICONS[runLight(run)] ?? 'circle-slash'
</script>

<script>
  // One agent's run history, newest first: each run is one row with a status
  // icon in its stoplight color, its title and when it started in local time.
  // A row opens the run's conversation. A failed run whose input core kept
  // for replay has a Retry action, which runs it again against the agent as
  // it is now. Failures core kept that the run list no longer holds join the
  // list as failed rows. `embedded` drops the heading and the section rule
  // for a panel that draws its own, such as the agent card's History.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { runOutcome, runTimeLabel, runTitle } from '../lib/agent-labels.js'
  import { loadReplays, replayRun } from './replays.js'

  let { tauri, agent, runs = [], limit = 20, embedded = false, disabled = false, onopen = null, onrun = () => {} } = $props()
  let replays = $state([])
  let busy = $state('')
  let error = $state('')
  let now = $state(Date.now())

  $effect(() => {
    agent.id
    void loadReplays(tauri, agent).then((answer) => { replays = answer.replays; error = answer.error })
  })
  // The day words move on while the profile stays open.
  $effect(() => {
    const timer = setInterval(() => { now = Date.now() }, 60_000)
    return () => clearInterval(timer)
  })

  const startedAt = (run) => run.created_at ?? run.updated_at
  const rows = $derived.by(() => {
    const known = new Set(runs.map((run) => run.id))
    const kept = replays.filter((replay) => !known.has(replay.replay_id)).map((replay) => ({ id: replay.replay_id, agent_id: replay.agent_id, thread_id: replay.thread_id ?? null, trigger: replay.trigger, status: 'failed', error: replay.error, created_at: replay.failed_at, runtimeId: agent.runtimeId ?? 'local' }))
    return [...runs, ...kept].sort((a, b) => Date.parse(startedAt(b)) - Date.parse(startedAt(a))).slice(0, limit)
  })
  const replayFor = (run) => run.status === 'failed' ? replays.find((replay) => replay.replay_id === run.id) : null

  async function retry(replay) {
    busy = replay.replay_id
    error = ''
    try {
      const answer = await replayRun(tauri, agent, replay)
      replays = replays.map((item) => item.replay_id === replay.replay_id ? { ...item, ...answer.replay } : item)
      if (answer.run) onrun(answer.run)
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = ''
    }
  }
</script>

<section class="history" class:embedded aria-label="History">
  {#if !embedded}<h3>History</h3>{/if}
  <ul aria-label="Runs">
    {#each rows as run (`${run.runtimeId ?? 'local'}:${run.id}`)}
      {@const title = runTitle(run, agent.routines)}
      {@const outcome = runOutcome(run)}
      {@const replay = replayFor(run)}
      {@const when = `${runTimeLabel(startedAt(run), now)}${outcome ? ` · ${outcome}` : ''}`}
      <li class="run" data-light={runLight(run)}>
        <button type="button" class="open" aria-label={`${title}, ${when}`} disabled={!onopen} onclick={() => onopen(run)}>
          <span class="light"><LucideIcon name={runIcon(run)} variant="action" size={14} /></span>
          <span class="text"><span class="title">{title}</span><time datetime={startedAt(run)}>{when}</time></span>
        </button>
        {#if replay}<button type="button" class="retry" aria-label={`Retry ${title}`} disabled={disabled || !!busy} onclick={() => retry(replay)}><LucideIcon name={busy === replay.replay_id ? 'loader-circle' : 'refresh-cw'} variant="action" size={14} /></button>{/if}
      </li>
    {:else}
      <li class="empty">No runs yet.</li>
    {/each}
  </ul>
  {#if error}<p role="alert">{error}</p>{/if}
</section>

<style>
  .history { display: grid; gap: 4px; padding: 10px 0; border-bottom: 1px solid var(--border); }
  .history.embedded { padding: 0; border-bottom: 0; }
  h3 { margin: 0 0 2px; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  ul { display: grid; margin: 0 -6px; padding: 0; list-style: none; }
  .embedded ul { margin: 0; }
  .embedded .open { padding: 5px 8px; }
  .run { position: relative; display: flex; align-items: center; min-width: 0; }
  .open { display: flex; flex: 1; align-items: flex-start; gap: 8px; min-width: 0; min-height: 36px; padding: 5px 6px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; text-align: left; cursor: pointer; }
  .open:hover:not(:disabled) { background: var(--faint); }
  .run:has(.retry) .open { padding-right: 32px; }
  .open:disabled { cursor: default; }
  .light { flex: none; display: inline-flex; padding-top: 2px; color: var(--warning); }
  .run[data-light="green"] .light { color: var(--success); }
  .run[data-light="red"] .light { color: var(--danger); }
  .text { display: grid; gap: 1px; min-width: 0; }
  .title { overflow: hidden; font-size: var(--text-13); text-overflow: ellipsis; white-space: nowrap; }
  time { color: var(--muted); font: var(--text-12) var(--font-human); }
  .retry { position: absolute; top: 50%; right: 4px; display: grid; place-items: center; width: 24px; height: 24px; padding: 0; transform: translateY(-50%); border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .retry:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .retry:disabled { opacity: .5; cursor: default; }
  .empty { padding: 4px 6px; color: var(--muted); font-size: var(--text-12); }
  [role="alert"] { margin: 0; color: var(--oxide); font-size: var(--text-12); }
  .embedded [role="alert"] { margin: 2px 8px; }
</style>

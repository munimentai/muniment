<script module>
  import { runActive, runLight } from '../lib/agent-labels.js'

  // A run's status icon, drawn in its stoplight color.
  const ICONS = { green: 'circle-check', red: 'circle-x' }
  export const runIcon = (run) => run.status === 'waiting_input' ? 'circle-alert' : runActive(run.status) ? 'loader-circle' : ICONS[runLight(run)] ?? 'circle-slash'
</script>

<script>
  // One agent's run history, newest first: each run is one row with a status
  // icon in its stoplight color, its title and when it started in local time.
  // A failed row also shows the reason the run failed.
  // A row opens the run's conversation. A failed run whose input core kept
  // for replay has a Retry action, which runs it again against the agent as
  // it is now. Failures core kept that the run list no longer holds join the
  // list as failed rows. `embedded` drops the heading and the section rule
  // for a panel that draws its own, such as the agent card's History.
  // Show older reads the next page of `agent.runs` from the agent's machine,
  // older than the oldest run listed, and shows `limit` more rows. `complete`
  // says `runs` already holds every run, so the list asks for no more.
  import { untrack } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { runOutcome, runTimeLabel, runTitle } from '../lib/agent-labels.js'
  import { runFailureText } from '../lib/run-failure.js'
  import { agentRequest, UNREACHABLE_LINE } from './agent-runtime.js'
  import { DELETED_THREAD_NOTE, threadDeleted } from '../lib/deleted-threads.svelte.js'
  import { loadReplays, replayRun } from './replays.js'

  // `connected` is false while this computer's runtime connection is down,
  // and the replays read again once it returns.
  let { tauri, agent, runs = [], limit = 20, complete = false, embedded = false, disabled = false, connected = true, onopen = null, onrun = () => {} } = $props()
  let replays = $state([])
  let busy = $state('')
  let error = $state('')
  let now = $state(Date.now())
  let shown = $state(untrack(() => limit))
  let older = $state([])
  let olderDone = $state(false)
  let loadingOlder = $state(false)

  $effect(() => {
    agent.id
    shown = untrack(() => limit)
    older = []
    olderDone = false
  })
  $effect(() => {
    agent.id
    if (!connected) return
    let alive = true
    // A read that still finds the runtime unreachable while the connection
    // is up shows no line. The read runs again when the connection returns.
    void loadReplays(tauri, agent).then((answer) => { if (alive) { replays = answer.replays; error = answer.error === UNREACHABLE_LINE && untrack(() => connected) ? '' : answer.error } })
    return () => { alive = false }
  })
  // The day words move on while the profile stays open.
  $effect(() => {
    const timer = setInterval(() => { now = Date.now() }, 60_000)
    return () => clearInterval(timer)
  })

  const startedAt = (run) => run.created_at ?? run.updated_at
  const listed = $derived.by(() => {
    const known = new Set(runs.map((run) => run.id))
    const read = older.filter((run) => !known.has(run.id))
    for (const run of read) known.add(run.id)
    const kept = replays.filter((replay) => !known.has(replay.replay_id)).map((replay) => ({ id: replay.replay_id, agent_id: replay.agent_id, thread_id: replay.thread_id ?? null, trigger: replay.trigger, status: 'failed', error: replay.error, created_at: replay.failed_at, runtimeId: agent.runtimeId ?? 'local' }))
    return [...runs, ...read, ...kept].sort((a, b) => Date.parse(startedAt(b)) - Date.parse(startedAt(a)))
  })
  const rows = $derived(listed.slice(0, shown))
  // The list asks for older runs once it shows a full page and the machine
  // may hold more than it has read.
  const moreToRead = $derived(!complete && !olderDone && runs.length + older.length >= limit)
  const hasOlder = $derived(listed.length > shown || moreToRead)

  async function showOlder() {
    if (loadingOlder) return
    error = ''
    if (moreToRead && listed.length <= shown + limit) {
      // Run ids sort by time, and core lists the runs older than the cursor id.
      const oldest = [...runs, ...older].map((run) => run.id).sort().at(0)
      loadingOlder = true
      try {
        const answer = await agentRequest(tauri, agent.runtimeId, 'agent.runs', { agent_id: agent.id, limit, cursor: oldest })
        older = [...older, ...(answer?.runs ?? []).map((run) => ({ ...run, runtimeId: agent.runtimeId ?? 'local' }))]
        olderDone = !answer?.next_cursor
      } catch (failure) {
        error = String(failure?.message ?? failure)
        return
      } finally {
        loadingOlder = false
      }
    }
    shown += limit
  }
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
      {@const gone = threadDeleted(run.thread_id)}
      <li class="run" data-light={runLight(run)}>
        <button type="button" class="open" aria-label={`${title}, ${when}${gone ? `. ${DELETED_THREAD_NOTE}` : ''}`} disabled={!onopen || gone} onclick={() => onopen(run)}>
          <span class="light"><LucideIcon name={runIcon(run)} variant="action" size={14} /></span>
          <span class="text"><span class="title">{title}</span><time datetime={startedAt(run)}>{when}</time>{#if run.status === 'failed' && runFailureText(run.error)}<span class="reason">{runFailureText(run.error)}</span>{/if}{#if gone}<span class="reason">{DELETED_THREAD_NOTE}</span>{/if}</span>
        </button>
        {#if replay}<button type="button" class="retry" aria-label={`Retry ${title}`} disabled={disabled || !!busy} onclick={() => retry(replay)}><LucideIcon name={busy === replay.replay_id ? 'loader-circle' : 'refresh-cw'} variant="action" size={14} /></button>{/if}
      </li>
    {:else}
      <li class="empty">No runs yet.</li>
    {/each}
  </ul>
  {#if hasOlder}<button type="button" class="older" disabled={loadingOlder} onclick={showOlder}>{loadingOlder ? 'Loading older runs…' : 'Show older'}</button>{/if}
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
  .reason { color: var(--muted); font: var(--text-12) var(--font-human); overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .retry { position: absolute; top: 50%; right: 4px; display: grid; place-items: center; width: 24px; height: 24px; padding: 0; transform: translateY(-50%); border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .retry:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .retry:disabled { opacity: .5; cursor: default; }
  .older { justify-self: start; padding: 4px 6px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-12) var(--font-human); cursor: pointer; }
  .older:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .older:disabled { cursor: default; }
  .empty { padding: 4px 6px; color: var(--muted); font-size: var(--text-12); }
  [role="alert"] { margin: 0; color: var(--danger); font-size: var(--text-12); }
  .embedded [role="alert"] { margin: 2px 8px; }
</style>
